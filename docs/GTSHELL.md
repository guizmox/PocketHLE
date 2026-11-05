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

## Bluetooth file transfer

Enable Classic Bluetooth in PocketHLE on both hosts before launching GTShell.
In the receiving shell, open **Bluetooth → Receive files** and leave it waiting.
On the sender, select an image and choose **Send → Bluetooth**, then select the
receiving host and accept the transfer there. Use the platform Bluetooth
permissions, discoverability and pairing steps in [GIZMONDO.md](GIZMONDO.md#bluetooth).
Check received images in the gallery or in `MyPictures` on the profile's Flash Disk.

GTShell uses BTDRT discovery and Winsock RFCOMM. PocketHLE translates discovery
addresses for the guest and resolves them back to host addresses when connecting.
The receiving host advertises the logical Gizmondo service UUID through SDP.
On Windows the OS allocates the physical RFCOMM channel.

The Windows-to-Android image transfer and failure on an interrupted transfer
have been verified. Android-to-Windows transfer still needs validation with
the Windows listener change. A host appearing in discovery only confirms that
its radio was found. Android can report the same read exception for a completed
transfer and an interrupted link. PocketHLE preserves the socket error in this
case so a real interruption still reaches the guest.

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

### CE shell compatibility APIs

`InternetSetOptionW` accepts the global `INTERNET_OPTION_SETTINGS_CHANGED`
notification (39). Backends retain the explicit settings of existing sessions;
there is no cached registry proxy configuration to invalidate. Other options
return an explicit unsupported-option error rather than false success.

`SendNotifyMessageW` invokes same-thread guest window procedures synchronously
and queues notifications for other window threads. Broadcast recipients are
snapshotted top-level windows. Pointer-bearing system messages are rejected.
`SHCreateShortcut` writes a CE text shortcut through VFS creation/sharing/quota
rules; it does not overwrite an existing shortcut or create missing directories.
The shortcut command uses the emulator's existing single-byte CP_ACP mapping
(Latin-1), including accented Western European characters. An unrepresentable
command fails with ERROR_NO_UNICODE_TRANSLATION before creating a file; it is
never silently written as incompatible UTF-8 or with replacement characters.

`AddFontResourceW` loads bounded TrueType/OpenType outlines and TrueType
collections from guest VFS. Registered faces participate in text rendering and
`EnumFontFamiliesW` callbacks, including early stop. Fonts and callback storage
belong to the process and are released at exit. Raster FNT/FON and FOT wrappers
are not supported. RemoveFontResourceW is outside this batch; registration lasts
until process exit. This is not a global host font installation. Enumeration
returns design-unit em size, height and average character width derived from
the selected font, independently of its requested pixel size. Callback scratch
storage is also reclaimed if its guest thread terminates before returning.

`RasGetEntryProperties`, `RasSetEntryProperties` and `RasSetEntryDialParams`
store CE-format phone-book data in the emulated registry. The supported CE
layouts are RASENTRY 3472 bytes, RASDIALPARAMS 1464 bytes and RASCONN 52 bytes,
not desktop Windows structures. Properties and opaque device configuration
persist through the normal registry lifecycle. Passwords remain session-only
and are excluded from persistent registry snapshots. `RasEnumConnections`
reports zero connections because this device has no RasDial transport; host
WinINet routing is not a dial-up connection.

`GetServiceHandle` reports absent services with INVALID_HANDLE_VALUE; in
particular, BTS1: is not the BTH1: stream driver. `DeregisterService` rejects
invalid/unowned service handles and never stops a host service. Loading native
services.exe service DLLs is outside this implementation.

Cryptographic providers, CEDB, TAPI provider registration and scheduled
application launch are separate work; these APIs do not imply their support.

The contributor reports desktop and Android shell operation. Integration tests
validate the changed code paths; complete shell workflows, media playback and
physical device compatibility still require runtime validation.
