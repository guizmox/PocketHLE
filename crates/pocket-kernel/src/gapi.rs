//! Canonical GAPI (`gx.dll`) key list and the host-side key mapping
//! that goes with it.
//!
//! A GAPI game does not compare `WM_KEYDOWN` against hard-coded Win32
//! virtual-key codes. It calls `GXGetDefaultKeys` once, copies the
//! returned `GXKeyList` into a global, and from then on only reacts to
//! the virtual keys that table names. Asphalt 2 3D is a textbook
//! example: its window procedure resolves the confirm/back/start
//! buttons through `keyList.vkA`, `keyList.vkB` and `keyList.vkStart`,
//! so a plain `VK_RETURN` press is silently dropped and the game never
//! leaves its language screen.
//!
//! Because of that, the codes below are effectively part of PocketHLE's
//! ABI: `gx.dll` hands them to the guest, the frontends synthesise them
//! for the on-screen buttons, and [`remap_host_key`] rewrites the host
//! keyboard's confirm key into `vkA`. Keeping them in one place is what
//! stops those three layers from drifting apart.

/// Win32 virtual-key codes, as documented in `winuser.h`.
pub const VK_UP: u16 = 0x26;
pub const VK_DOWN: u16 = 0x28;
pub const VK_LEFT: u16 = 0x25;
pub const VK_RIGHT: u16 = 0x27;
/// Classic Pocket PC hardware reports its face buttons as
/// `VK_APP1..VK_APP4`-style codes in the `0xD1..=0xD4` range.
pub const VK_A: u16 = 0xD1;
pub const VK_B: u16 = 0xD2;
pub const VK_C: u16 = 0xD3;
pub const VK_START: u16 = 0xD4;

/// `VK_RETURN` — what a host keyboard sends for "confirm", and what
/// Smartphone builds call `VK_TACTION`.
pub const VK_RETURN: u16 = 0x0D;

/// Size of a `GXKeyList` as declared in the PPC2003 SDK's `gx.h`:
/// eight `{SHORT vkXxx; POINT ptXxx;}` entries, each padded to 12
/// bytes so the `POINT` stays 4-aligned.
pub const KEY_LIST_BYTES: usize = 0x60;

/// The `GXKeyList` in `gx.h` field order: `vkUp`, `vkDown`, `vkLeft`,
/// `vkRight`, `vkA`, `vkB`, `vkC`, `vkStart`.
pub const DEFAULT_KEYS: [u16; 8] = [
    VK_UP, VK_DOWN, VK_LEFT, VK_RIGHT, VK_A, VK_B, VK_C, VK_START,
];

/// Serialise [`DEFAULT_KEYS`] into the exact `GXKeyList` layout the
/// guest expects. The `POINT` members stay zero: they only matter for
/// devices that emulate the D-pad with stylus regions, and reporting
/// (0,0) is how a real device says "this button is a real button".
pub fn default_key_list() -> [u8; KEY_LIST_BYTES] {
    key_list_with_action(VK_A)
}

/// Device-selected action key; the Gizmondo/legacy profile keeps VK_A.
pub fn key_list_with_action(action: u16) -> [u8; KEY_LIST_BYTES] {
    let mut keys = DEFAULT_KEYS;
    keys[4] = action;
    let mut buf = [0u8; KEY_LIST_BYTES];
    for (i, vk) in keys.iter().enumerate() {
        buf[i * 12..i * 12 + 2].copy_from_slice(&vk.to_le_bytes());
    }
    buf
}

/// Rewrite a host virtual key into the code a GAPI guest listens for.
///
/// Only `VK_RETURN` is touched, and only once the guest has actually
/// asked for the key list. A title that reads `VK_RETURN` directly
/// never calls `GXGetDefaultKeys`, so it keeps seeing the real code.
pub fn remap_host_key(vk: u16, gapi_keys_queried: bool) -> u16 {
    remap_host_key_with_action(vk, gapi_keys_queried, VK_A)
}

/// Keep the delivered action key consistent with GXGetDefaultKeys.
pub fn remap_host_key_with_action(vk: u16, queried: bool, action: u16) -> u16 {
    if queried && vk == VK_RETURN {
        action
    } else {
        vk
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_list_has_gx_h_layout() {
        let buf = default_key_list();
        for (i, vk) in DEFAULT_KEYS.iter().enumerate() {
            assert_eq!(u16::from_le_bytes([buf[i * 12], buf[i * 12 + 1]]), *vk);
            // Two bytes of padding, then a zeroed POINT.
            assert_eq!(&buf[i * 12 + 2..i * 12 + 12], &[0u8; 10]);
        }
    }

    #[test]
    fn return_becomes_a_only_for_gapi_guests() {
        assert_eq!(remap_host_key(VK_RETURN, true), VK_A);
        assert_eq!(remap_host_key(VK_RETURN, false), VK_RETURN);
        assert_eq!(remap_host_key(VK_LEFT, true), VK_LEFT);
        assert_eq!(remap_host_key(VK_A, true), VK_A);
    }
    #[test]
    fn pocket_pc_action_and_legacy_gizmondo_mapping_are_separate() {
        let legacy = default_key_list();
        let ppc = key_list_with_action(VK_RETURN);
        assert_eq!(&ppc[..48], &legacy[..48]);
        assert_eq!(&ppc[50..], &legacy[50..]);
        assert_eq!(u16::from_le_bytes([ppc[48], ppc[49]]), VK_RETURN);
        for vk in [
            VK_RETURN, VK_UP, VK_DOWN, VK_LEFT, VK_RIGHT, VK_A, VK_B, VK_C, VK_START, 0x11, 0x20,
            0x10, 0x09, 0x1b, 0x70, 0x7a,
        ] {
            assert_eq!(
                remap_host_key_with_action(vk, true, VK_A),
                remap_host_key(vk, true)
            );
            assert_eq!(remap_host_key_with_action(vk, true, VK_RETURN), vk);
            assert_eq!(remap_host_key_with_action(vk, false, VK_RETURN), vk);
        }
    }
}
