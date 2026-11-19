# PocketPC startup, framebuffer copies and Android controls

Apply these replacement files to `snapshots` commit
`b24d80122d4dc7607dce69e1a8348f918b75a99d` (snapshots phase 6), preserving paths.
`patch-files.txt` lists the included paths. `remove-files.txt` is empty: this
increment does not remove source files. Game binaries and data are not included.

## Changes

- Port the earlier generic DOS short-path fix into the current VFS, retaining
  snapshot code and canonical file/overlay/sharing identities.
- Model automatic native fullscreen coverage separately from explicit
  SHFullScreen show/hide requests. This removes the synthetic strip that caused
  Brothers in Arms to pause itself, while preserving MoonGBA's work-area chrome.
- Coalesce partial CPU GAPI copies using the existing display poll, updating only
  the copied bytes instead of reading and publishing the full panel per scanline.
  Explicit GX, DirectDraw locks, rotated/padded storage and GPU paths retain their
  existing presentation boundaries. Temporary profiling code is not included.
- Reserve a safety gap around Android S/L controls. Shorten the Classic action
  rows; lower/shorten Controller P keys, lower the D-pad/stick and bring the face
  diamond closer to the stick. Keep the top toolbar and portrait layouts stable.

The audio preview unit test also avoids a wall-clock-dependent assertion;
production audio is unchanged.

The WinCE snapshot section is now version 9 to capture the explicit shell state.
Recreate development snapshots; no legacy compatibility reader is included.
The generic DOS-path port does not replace the VFS with the old pre-snapshot file.

## Validation and remaining work

See `docs/testing/POCKETPC-GAPI-2026-11-19.md` for measurements and validation,
`docs/testing/POCKETPC-STARTUP-2026-11-19.md` for startup evidence and remaining
V-Rally MFC ABI work. Prey's measured process CPU cost falls by about 30% in the
host gameplay sample; this is not a measured Android FPS gain. Physical Android
and Windows validation remain separate from the host/ABI build checks.
