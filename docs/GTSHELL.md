# Gizmondo shell profiles

GTShell support is integrated into the normal PocketHLE desktop and Android
applications. Use the standard build and update channels; no separate application
ID or fork release endpoint is required. See [Android build](ANDROID-BUILD.md)
and the root [README](../README.md).

## User-provided files

The shell requires its original Flash Disk and Flash Disk2 contents, including
its executable, native MFC/PMS DLLs, skins and language data. An SD card is optional.
These files are supplied by the user and are not distributed with PocketHLE.
Create a version-1 backup profile with:

```sh
python3 tools/make-gtshell-profile.py --flash-disk "device/flash disk" \
  --flash-disk2 "device/flash disk2" --sd-card "device/sd card" \
  --output "gtshell-profile.zip"
```

Select a different executable explicitly with `--gtshell PATH` when needed.
Use physical directories, not symlinks or Windows junctions. Restore the ZIP
through the normal backup menu. Merge without overwrite keeps existing settings.
Do not restore with replacement over a library you intend to preserve.

`GameSettings.storage_card_dir` optionally selects the SD Card/Storage Card root.
Relative paths resolve against the library root; missing explicit roots fail
before execution. The executable installation path remains Flash Disk2. Explicit
cards expose their real Game directory instead of the imported-game alias.
Foreground SDCreateProcess children must remain within the canonical card root;
concurrent CreateProcess retains the existing VFS-resolved launch contract.
Synchronous children preserve the existing host-session suspend/resume guard.

## Implementation and limits

The per-process kernel MSXML arena implements the shell's read-only DOM operations,
with synthetic MSXML3 vtables and owned BSTRs. Reads are synchronous through VFS,
including sharing rules and overlays. UTF-8 and BOM UTF-16 loads are bounded to
8 MiB, 65536 nodes and depth 256; DTDs/external entities are rejected. Unsupported
DOM operations and async TRUE return E_NOTIMPL. This is not a complete MSXML
implementation. Other COM classes keep their existing dispatch path.

The entry classifier recognizes ARM `mov r0, #constant; bx lr` leaf entry points
even when the PE machine tag says THUMB; genuine Thumb entry behavior is retained.
No user DLL or EXE bytes are changed.

The contributor reports desktop and Android shell operation. Integration tests
validate the changed code paths; complete shell workflows, media playback and
physical device compatibility still require runtime validation.
