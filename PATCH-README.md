# Snapshot phase 7 — cumulative source patch

Base: 9651be14f1f24f9779c22a3c5dd5dd6b63b962c7 (snapshots).
This archive includes phase 6. Apply it instead of the earlier phase 6 archive.

Copy the paths in patch-files.txt over the checkout, preserving directories.
Delete the obsolete path listed in remove-files.txt. No assets, generated native
libraries, APKs, build tooling or saved slots are included.

Recreate development snapshots. No reader for the old development schemas is
kept. See docs/SNAPSHOTS.md and docs/testing/SNAPSHOT-PHASE7-2026-11-19.md for
architecture, completed tests and remaining explicit limitations.

Use the repository's normal Desktop/Android build instructions after applying.
Android native libraries must be rebuilt for both ABIs before packaging the APK.
