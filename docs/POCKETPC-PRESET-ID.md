# Per-game PocketPC Preset ID

A PocketPC DEVICE_ID contains independent Preset and Platform byte arrays.
Some old registrations use Preset bytes, not the owner name or Platform ID.

Stop the game, open its settings, then use **Preset ID override (hexadecimal)**.
Enter opaque bytes as contiguous hexadecimal pairs. Empty keeps the previous
ASCII device name/default behavior. This override takes precedence over the
ASCII Device name for DEVICE_ID only; it does not rename the device registry.
The Platform ID and owner settings remain independent. Apply at next launch.

The override is process-local and never replaces the shared device identity.
It is ignored for Gizmondo. Restore the empty field to remove the override.
Existing settings and backups default to empty; Android preserves the new field
when saving other settings.

The IOCTL handler additionally accepts a 20-byte header-only size negotiation,
returns the required size in dwSize without touching the payload, and clears
its probe error on a successful device-ID response. This is a compatibility
extension for callers that do not use the documented zero-dwSize query.
No game executable or registration check is modified. These settings are not
an automatic unlock: use the registration code associated with your identity.

Validation: isolated original ARM routines for a supplied purchased game;
156 library tests, 591 API tests (4 GL tests ignored), and process-local identity
persistence regression. Full Android/desktop native builds and device gameplay
remain to be checked. No build dependencies or lockfile changes.
