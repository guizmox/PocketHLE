# VFSTEST v2 — ARM virtual filesystem diagnostic

Import `PocketHLE-VFSTEST-v2.zip` into PocketHLE and run it twice. Each time, wait
for **SUCCES** (the existing guest diagnostic message), then press Enter to close
the dialog. The same program tests files, volumes and interprocess operations.

The report is `\Flash Disk\VFSTEST.TXT`, normally
`C:\Users\gtristant\Documents\PocketHLE\flash\VFSTEST.TXT`:

```text
VFSTEST_RESULT PASS checks=0x00000067 failures=0x00000000
```

The 103 checks cover CreateFileW dispositions, CRT r+/w+/a/a+ modes, local/remote
sharing and duplication, errors and invalid/protected output pointers, searches
and attributes, actual directory deletion, moves without overwriting and negative
file-position changes. They also check shared RAM files, volume accounting, the
32 MiB Flash quota, refusal to write when full, and space reclaimed by truncation
or deletion without charging NAND storage against RAM.

The diagnostic uses only its `\Flash Disk\PocketHLE-VFS-PROBE` directory and report.
If the directory already exists, it refuses to start to avoid deleting unknown
files. On success, it removes its temporary files/directories and keeps only the
report. An existing report is replaced. The card's `asset.bin` checks read-only
mount behavior. Game saves are not used.

The 64 MiB NAND model reserves 32 MiB for the OS and provides 32 MiB for Flash Disk.
Storage capacity is separate from the RAM budget. The SD volume has a synthetic
capacity rounded up to a power of two, at least 64 MiB, according to its content;
it does not detect physical card capacity. Additional attributes are shared during
a session; full persistence across restarts is not implemented. This diagnostic
validates audited faults and does not cover every WinCE file API.

Build without the proprietary SDK:

```text
python tools/vfstest/build.py --clang clang --lld ld.lld
```

Output: `tools/vfstest/dist/PocketHLE-VFSTEST.zip`.
The native regression executes ARM twice and checks cleanup and RAM accounting:

```text
cargo test -p pocket-core --no-default-features --features unicorn --test ram_guest native_arm_vfs -- --nocapture
```

Reference WinCE contracts:

- https://learn.microsoft.com/en-us/previous-versions/ms959950(v=msdn.10)
- https://learn.microsoft.com/en-us/previous-versions/ms961237(v=msdn.10)
- https://learn.microsoft.com/en-us/previous-versions/windows/embedded/ms891933(v=msdn.10)
- https://learn.microsoft.com/en-us/previous-versions/ms890887(v=msdn.10)

Version 2 retains all 103 checks and the strict quota.nand_not_ram comparison.
It initializes both GlobalMemoryStatus buffers before measuring and adds
INFO quota.ram_before / ram_after / total_before / total_after to the report.
This distinguishes RAM page changes from NAND charges; no tolerance turns a
failed assertion into a pass.
