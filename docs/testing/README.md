# Validation evidence index

These are dated records of particular revisions, binaries and test hosts. They
retain the observations and test counts of those runs. They are not current
setup instructions, feature inventories or proof that every later build passes.
Use the [documentation index](../README.md) for current owning guides.

| Campaign | Records | Current contract |
| --- | --- | --- |
| PocketPC startup and GAPI input | [Startup](POCKETPC-STARTUP-2026-11-19.md), [GAPI](POCKETPC-GAPI-2026-11-19.md) | [Architecture](../AGENTS.md), [Graphics](../GPU.md) |
| Snapshot checkpoints and compression | [Checkpoints](SNAPSHOT-CHECKPOINTS-2026-11-17.md), [Compression](SNAPSHOT-COMPRESSED-2026-11-19.md) | [Snapshots](../SNAPSHOTS.md) |
| Snapshot implementation stages | [Phase 3](SNAPSHOT-PHASE3-2026-11-18.md), [Phase 4](SNAPSHOT-PHASE4-2026-11-18.md), [Phase 5](SNAPSHOT-PHASE5-2026-11-18.md), [Phase 6](SNAPSHOT-PHASE6-2026-11-19.md), [Phase 7](SNAPSHOT-PHASE7-2026-11-19.md) | [Snapshots](../SNAPSHOTS.md), [WinCE records](../SNAPSHOT-WINCE-FORMAT.md) |
| Ultima paint/input and early MFC archive support | [Ultima](ULTIMA-2026-11-20.md) | [MFC](../MFC.md), [Graphics](../GPU.md) |
| V-Rally restore and temporary archives | [V-Rally](VRALLY-2026-11-20.md) | [Snapshots](../SNAPSHOTS.md) |

Some historical campaigns predate WinCE container 12 or the expanded MFC bridge.
Their old section numbers and unsupported-feature lists describe the tested
revision. The current [WinCE registry](../SNAPSHOT-WINCE-FORMAT.md) and
[MFC guide](../MFC.md) take precedence; do not install an archived patch or
reintroduce removed settings to reproduce those instructions on a current build.

Additional retained evidence lives in the [proof index](../../proof/README.md),
[validation artifacts](../../validation/README.md), [historical validation
notes](../history/VALIDATION.md) and [graphics excerpts](../history/GRAPHICS-EVIDENCE.md).
A documentation edit does not constitute a new gameplay or device-validation run.
