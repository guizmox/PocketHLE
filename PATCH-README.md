# Snapshot source update

Base: `d1552d659f5a6b14d02a52ceb5528022dd08422c` on `snapshots`.

Copy the files in `patch-files.txt` over this checkout, preserving directories.
Delete all paths listed in `remove-files.txt`. The archive contains source and
documentation only. Rebuild Android native libraries for both ABIs before APK
packaging, using the repository's normal build instructions.

See `docs/SNAPSHOTS.md` and
`docs/testing/SNAPSHOT-COMPRESSED-2026-11-19.md` for the current behavior,
validation and limitations. Snapshot payload schemas are unchanged by this patch.
