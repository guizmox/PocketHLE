//! Persistent host-key → Windows Mobile button map.
//!
//! The launchers present the guest with a fixed set of *buttons* — the
//! Pocket PC D-pad, the three face buttons a GAPI title asks
//! `GXGetDefaultKeys` about, the two soft keys, and the Gizmondo turbo
//! key. Which host key produces which button is a user preference, so
//! it lives here, in the same `config.json` the rest of the launcher
//! settings do, rather than in a hard-coded table inside one frontend.
//!
//! Host keys are stored by *name*, not by keycode: `"Left"`,
//! `"Space"`, `"1"`. The names are exactly `egui::Key::name()` (and
//! `egui::Key::from_name` parses them back), which is what the desktop
//! launcher's keyboard handler works in. Storing names rather than
//! numbers keeps `config.json` readable and survives an egui upgrade
//! renumbering its enum.
//!
//! egui spells the arrow keys `"Up"`/`"Down"`/`"Left"`/`"Right"` in
//! `name()` but also parses `"ArrowUp"` and friends in `from_name`, so
//! [`keys_match`] treats the two spellings as the same key. Otherwise a
//! `config.json` hand-edited (or written by a build that stored the
//! `ArrowUp` spelling) would silently lose its D-pad.
//!
//! The virtual-key codes each button sends are *not* a preference —
//! they are the codes `gx.dll` hands the guest, so they have to match
//! `pocket_core::kernel::gapi` and the on-screen pads in both
//! frontends. See [`GuestButton::vk`].

use serde::{Deserialize, Serialize};

/// Win32 virtual-key codes, mirrored from `pocket_kernel::gapi` and
/// `winuser.h`. This crate deliberately does not depend on the kernel
/// (the Android launcher links it without the emulator), so the codes
/// are repeated here; `guest_button_vks_match_the_gapi_table` in
/// `pocket-desktop` pins them against the real table.
mod vk {
    pub const UP: u16 = 0x26;
    pub const DOWN: u16 = 0x28;
    pub const LEFT: u16 = 0x25;
    pub const RIGHT: u16 = 0x27;
    pub const RETURN: u16 = 0x0D;
    pub const SPACE: u16 = 0x20;
    pub const TAB: u16 = 0x09;
    pub const SHIFT: u16 = 0x10;
    pub const CTRL: u16 = 0x11;
    pub const ESCAPE: u16 = 0x1B;
    pub const F1: u16 = 0x70;
    pub const F2: u16 = 0x71;
    pub const F3: u16 = 0x72;
    pub const F4: u16 = 0x73;
    pub const F11: u16 = 0x7A;
}

/// One button on the emulated device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestButton {
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    /// The confirm key. `VK_RETURN` on the wire, rewritten to `vkA` for
    /// a guest that asked for the GAPI key list — see
    /// `pocket_kernel::gapi::remap_host_key`.
    Action,
    ButtonA,
    ButtonB,
    ButtonC,
    Soft1,
    Soft2,
    /// Legacy PocketHLE turbo helper. Not a physical Gizmondo button.
    Turbo,
    /// Gizmondo top-row ("piano") hardware buttons.
    GizPiano1,
    GizPiano2,
    GizPiano3,
    GizPiano4,
    GizPiano5,
    /// Pocket PC application hardware keys (VK_APP1..VK_APP4).
    PocketPcApp1,
    PocketPcApp2,
    PocketPcApp3,
    PocketPcApp4,
}

impl GuestButton {
    /// Every button, in the order the settings UI lists them.
    pub const ALL: [GuestButton; 20] = [
        GuestButton::DpadUp,
        GuestButton::DpadDown,
        GuestButton::DpadLeft,
        GuestButton::DpadRight,
        GuestButton::Action,
        GuestButton::ButtonA,
        GuestButton::ButtonB,
        GuestButton::ButtonC,
        GuestButton::Soft1,
        GuestButton::Soft2,
        GuestButton::Turbo,
        GuestButton::GizPiano1,
        GuestButton::GizPiano2,
        GuestButton::GizPiano3,
        GuestButton::GizPiano4,
        GuestButton::GizPiano5,
        GuestButton::PocketPcApp1,
        GuestButton::PocketPcApp2,
        GuestButton::PocketPcApp3,
        GuestButton::PocketPcApp4,
    ];

    /// The virtual-key code this button sends into the guest.
    pub fn vk(self) -> u16 {
        match self {
            GuestButton::DpadUp => vk::UP,
            GuestButton::DpadDown => vk::DOWN,
            GuestButton::DpadLeft => vk::LEFT,
            GuestButton::DpadRight => vk::RIGHT,
            GuestButton::Action => vk::RETURN,
            GuestButton::ButtonA => vk::CTRL,
            GuestButton::ButtonB => vk::SPACE,
            GuestButton::ButtonC => vk::SHIFT,
            GuestButton::Soft1 => vk::TAB,
            GuestButton::Soft2 => vk::ESCAPE,
            GuestButton::Turbo => vk::F3,
            GuestButton::GizPiano1 => vk::F1,
            GuestButton::GizPiano2 => vk::F2,
            GuestButton::GizPiano3 => vk::F3,
            GuestButton::GizPiano4 => vk::F4,
            // Official Gizmondo Keys SDK: Piano 5 is VK_F11, not VK_F5.
            GuestButton::GizPiano5 => vk::F11,
            GuestButton::PocketPcApp1 => 0xC1,
            GuestButton::PocketPcApp2 => 0xC2,
            GuestButton::PocketPcApp3 => 0xC3,
            GuestButton::PocketPcApp4 => 0xC4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            GuestButton::DpadUp => "D-pad up / Gizmondo ↑",
            GuestButton::DpadDown => "D-pad down / Gizmondo ↓",
            GuestButton::DpadLeft => "D-pad left / Gizmondo ←",
            GuestButton::DpadRight => "D-pad right / Gizmondo →",
            GuestButton::Action => "Action / Gizmondo South (Play)",
            GuestButton::ButtonA => "Button A / Gizmondo North (Stop)",
            GuestButton::ButtonB => "Button B / Gizmondo East (Forward)",
            GuestButton::ButtonC => "Button C / Gizmondo West (Rewind)",
            GuestButton::Soft1 => "Soft key 1 / Gizmondo L",
            GuestButton::Soft2 => "Soft key 2 / Gizmondo R",
            GuestButton::Turbo => "Turbo",
            GuestButton::GizPiano1 => "Gizmondo Home (Piano 1)",
            GuestButton::GizPiano2 => "Gizmondo Volume (Piano 2)",
            GuestButton::GizPiano3 => "Gizmondo Brightness (Piano 3)",
            GuestButton::GizPiano4 => "Gizmondo Alert (Piano 4)",
            GuestButton::GizPiano5 => "Gizmondo Power (Piano 5)",
            GuestButton::PocketPcApp1 => "Pocket PC hardware button 1",
            GuestButton::PocketPcApp2 => "Pocket PC hardware button 2",
            GuestButton::PocketPcApp3 => "Pocket PC hardware button 3",
            GuestButton::PocketPcApp4 => "Pocket PC hardware button 4",
        }
    }

    /// Host keys bound to this button out of the box.
    ///
    /// Defaults shared by PocketPC and the Gizmondo skin. The guest VKs stay
    /// compatible with GAPI; only the host-side shortcuts are user preferences.
    pub fn default_keys(self) -> &'static [&'static str] {
        match self {
            // Defaults are chosen for the Gizmondo skin while retaining the
            // same guest VKs used by PocketPC/GAPI titles.
            GuestButton::DpadUp => &["Z"],
            GuestButton::DpadDown => &["S"],
            GuestButton::DpadLeft => &["Q"],
            GuestButton::DpadRight => &["D"],
            GuestButton::Action => &["Down"],
            GuestButton::ButtonA => &["Up"],
            GuestButton::ButtonB => &["Right"],
            GuestButton::ButtonC => &["Left"],
            GuestButton::Soft1 => &["1"],
            GuestButton::Soft2 => &["2"],
            GuestButton::Turbo => &["T"],
            GuestButton::GizPiano1 => &["F1"],
            GuestButton::GizPiano2 => &["F2"],
            GuestButton::GizPiano3 => &["F3"],
            GuestButton::GizPiano4 => &["F4"],
            GuestButton::GizPiano5 => &["F5"],
            // Opt-in bindings: never steal an existing keyboard/controller key.
            GuestButton::PocketPcApp1
            | GuestButton::PocketPcApp2
            | GuestButton::PocketPcApp3
            | GuestButton::PocketPcApp4 => &[],
        }
    }
}

/// Whether two stored host-key names refer to the same key.
///
/// Case-insensitive, and tolerant of the two spellings egui accepts for
/// the arrow keys (`name()` yields `"Up"`, `from_name` also parses
/// `"ArrowUp"`).
pub fn keys_match(a: &str, b: &str) -> bool {
    canonical_key_name(a).eq_ignore_ascii_case(canonical_key_name(b))
}

fn canonical_key_name(key: &str) -> &str {
    for arrow in ["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"] {
        if key.eq_ignore_ascii_case(arrow) {
            return &arrow["Arrow".len()..];
        }
    }
    key
}

/// One button and the host keys bound to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyBinding {
    pub button: GuestButton,
    /// Host key names (`egui::Key::name()`). Empty means "this button
    /// has no keyboard binding", which is a legitimate choice.
    #[serde(default)]
    pub keys: Vec<String>,
}

/// The whole map. Serialised as a list rather than a JSON object so the
/// Android side can round-trip it with `org.json` without having to
/// know the button names, and so the order the settings screen shows is
/// stable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct KeyBindings {
    pub bindings: Vec<KeyBinding>,
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            bindings: GuestButton::ALL
                .iter()
                .map(|&button| KeyBinding {
                    button,
                    keys: button
                        .default_keys()
                        .iter()
                        .map(|k| (*k).to_string())
                        .collect(),
                })
                .collect(),
        }
    }
}

impl KeyBindings {
    /// Host keys currently bound to `button`.
    pub fn keys_for(&self, button: GuestButton) -> &[String] {
        self.bindings
            .iter()
            .find(|b| b.button == button)
            .map(|b| b.keys.as_slice())
            .unwrap_or(&[])
    }

    /// Replace the keys bound to `button`, dropping duplicates and
    /// stealing each key from whichever button held it before. A host
    /// key that produced two different guest buttons would send both on
    /// every press, which is never what the user meant.
    pub fn set_keys(&mut self, button: GuestButton, keys: Vec<String>) {
        let mut deduped: Vec<String> = Vec::with_capacity(keys.len());
        for key in keys {
            if !deduped.iter().any(|k| keys_match(k, &key)) {
                deduped.push(key);
            }
        }
        for other in self.bindings.iter_mut() {
            if other.button != button {
                other
                    .keys
                    .retain(|k| !deduped.iter().any(|d| keys_match(d, k)));
            }
        }
        match self.bindings.iter_mut().find(|b| b.button == button) {
            Some(existing) => existing.keys = deduped,
            None => self.bindings.push(KeyBinding {
                button,
                keys: deduped,
            }),
        }
    }

    /// Bind one more host key to `button`.
    pub fn bind(&mut self, button: GuestButton, key: &str) {
        let mut keys = self.keys_for(button).to_vec();
        keys.push(key.to_string());
        self.set_keys(button, keys);
    }

    /// Drop one host key from `button`.
    pub fn unbind(&mut self, button: GuestButton, key: &str) {
        let keys: Vec<String> = self
            .keys_for(button)
            .iter()
            .filter(|k| !keys_match(k, key))
            .cloned()
            .collect();
        self.set_keys(button, keys);
    }

    /// The button a host key produces, if any. This is the lookup the
    /// desktop launcher's keyboard handler does on every key event.
    pub fn button_for_key(&self, key: &str) -> Option<GuestButton> {
        self.bindings
            .iter()
            .find(|b| b.keys.iter().any(|k| keys_match(k, key)))
            .map(|b| b.button)
    }

    /// The virtual key a host key sends into the guest, if any.
    pub fn vk_for_key(&self, key: &str) -> Option<u16> {
        self.button_for_key(key).map(GuestButton::vk)
    }

    /// Add any button missing from a config written by an older build,
    /// so a new button does not silently arrive unbound.
    pub fn fill_missing_buttons(&mut self) {
        for button in GuestButton::ALL {
            if !self.bindings.iter().any(|b| b.button == button) {
                // Use the normal binding path so a newly introduced default
                // (notably Gizmondo Brightness = host F3) cannot leave the
                // same host key attached to an older control such as Turbo.
                let keys = button
                    .default_keys()
                    .iter()
                    .map(|k| (*k).to_string())
                    .collect();
                self.set_keys(button, keys);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn pocket_pc_buttons_do_not_steal_legacy_bindings_and_survive_settings() {
        let mut bindings = super::KeyBindings::default();
        let before: Vec<_> = super::GuestButton::ALL[..16]
            .iter()
            .map(|b| (*b, bindings.keys_for(*b).to_vec()))
            .collect();
        bindings.fill_missing_buttons();
        for (button, keys) in before {
            assert_eq!(bindings.keys_for(button), keys);
        }
        for button in super::GuestButton::ALL[16..].iter() {
            assert!(bindings.keys_for(*button).is_empty());
        }
        bindings.bind(super::GuestButton::PocketPcApp3, "F8");
        bindings.bind(super::GuestButton::PocketPcApp4, "F9");
        let json = serde_json::to_string(&bindings).unwrap();
        let restored: super::KeyBindings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.vk_for_key("F8"), Some(0xc3));
        assert_eq!(restored.vk_for_key("F9"), Some(0xc4));
        let mut pad = super::xbox_gamepad_bindings();
        pad.insert("South".into(), super::GuestButton::PocketPcApp3);
        let json = serde_json::to_string(&pad).unwrap();
        let restored: std::collections::BTreeMap<String, super::GuestButton> =
            serde_json::from_str(&json).unwrap();
        assert_eq!(restored["South"].vk(), 0xc3);
    }

    use super::*;

    #[test]
    fn defaults_cover_every_button() {
        let bindings = KeyBindings::default();
        assert_eq!(bindings.bindings.len(), GuestButton::ALL.len());
        for button in GuestButton::ALL[..16].iter().copied() {
            assert!(
                !bindings.keys_for(button).is_empty(),
                "{button:?} has no default key"
            );
        }
    }

    #[test]
    fn a_key_only_ever_drives_one_button() {
        let mut bindings = KeyBindings::default();
        // "Left" starts on DpadLeft; moving it to DpadRight has to
        // take it away from DpadLeft, or one press would send both.
        bindings.bind(GuestButton::DpadRight, "Left");
        assert_eq!(
            bindings.button_for_key("Left"),
            Some(GuestButton::DpadRight)
        );
        assert!(!bindings
            .keys_for(GuestButton::DpadLeft)
            .iter()
            .any(|key| key.eq_ignore_ascii_case("Left")));
    }

    #[test]
    fn lookup_is_case_insensitive_and_maps_to_vk() {
        let bindings = KeyBindings::default();
        assert_eq!(bindings.vk_for_key("z"), Some(0x26));
        // The arrow keys are the Gizmondo face buttons in the default layout.
        assert_eq!(bindings.vk_for_key("ArrowUp"), Some(0x11));
        assert_eq!(bindings.vk_for_key("F3"), Some(0x72));
        // Host F5 drives Piano 5, whose guest-side code is VK_F11.
        assert_eq!(bindings.vk_for_key("F5"), Some(0x7A));
        assert_eq!(bindings.vk_for_key("F12"), None);
    }

    #[test]
    fn unbinding_leaves_the_button_present_but_empty() {
        let mut bindings = KeyBindings::default();
        for key in bindings.keys_for(GuestButton::Turbo).to_vec() {
            bindings.unbind(GuestButton::Turbo, &key);
        }
        assert!(bindings.keys_for(GuestButton::Turbo).is_empty());
        assert!(bindings
            .bindings
            .iter()
            .any(|b| b.button == GuestButton::Turbo));
    }

    #[test]
    fn a_config_from_an_older_build_gains_the_new_buttons() {
        let mut bindings = KeyBindings {
            bindings: vec![KeyBinding {
                button: GuestButton::DpadUp,
                keys: vec!["K".to_string()],
            }],
        };
        bindings.fill_missing_buttons();
        assert_eq!(bindings.bindings.len(), GuestButton::ALL.len());
        assert_eq!(bindings.keys_for(GuestButton::DpadUp), &["K".to_string()]);
    }

    #[test]
    fn round_trips_through_json_as_a_list() {
        let bindings = KeyBindings::default();
        let json = serde_json::to_string(&bindings).unwrap();
        assert!(json.starts_with('['), "expected a JSON array, got {json}");
        let back: KeyBindings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, bindings);
    }
}

/// Position-based names remain stable across controller brands. Nintendo's
/// South button is B, East is A, North is X and West is Y.
fn xbox_gamepad_bindings() -> std::collections::BTreeMap<String, GuestButton> {
    use GuestButton::*;
    [
        ("DPadUp", DpadUp),
        ("DPadDown", DpadDown),
        ("DPadLeft", DpadLeft),
        ("DPadRight", DpadRight),
        ("LeftStickUp", DpadUp),
        ("LeftStickDown", DpadDown),
        ("LeftStickLeft", DpadLeft),
        ("LeftStickRight", DpadRight),
        ("South", Action),
        ("North", ButtonA),
        ("East", ButtonB),
        ("West", ButtonC),
        ("LeftTrigger", Soft1),
        ("RightTrigger", Soft2),
        ("LeftTrigger2", Turbo),
        ("Start", GizPiano1),
    ]
    .into_iter()
    .map(|(name, button)| (name.to_owned(), button))
    .collect()
}

pub fn default_gamepad_bindings() -> std::collections::BTreeMap<String, GuestButton> {
    if cfg!(feature = "steam-deck") {
        steam_deck_gamepad_bindings()
    } else {
        xbox_gamepad_bindings()
    }
}

pub fn steam_deck_gamepad_bindings() -> std::collections::BTreeMap<String, GuestButton> {
    let mut bindings = xbox_gamepad_bindings();
    bindings.remove("LeftTrigger"); // L1 is a host action, never a guest key.
    bindings.insert("RightTrigger".into(), GuestButton::GizPiano1);
    bindings.insert("LeftTrigger2".into(), GuestButton::Soft1);
    bindings.insert("RightTrigger2".into(), GuestButton::Soft2);
    bindings.insert("Start".into(), GuestButton::GizPiano2);
    bindings
}

/// Platform-specific Deck bindings override only the newly assigned controls.
/// All other Gizmondo controls keep the user's saved mapping.
pub fn steam_deck_guest_button(control: &str, gizmondo: bool) -> Option<GuestButton> {
    use GuestButton::*;
    if gizmondo {
        return (control == "LeftTrigger").then_some(GizPiano2);
    }
    match control {
        "RightTrigger2" => Some(Action),
        "LeftTrigger2" => Some(PocketPcApp2),
        "RightTrigger" => Some(PocketPcApp3),
        "LeftTrigger" => Some(PocketPcApp4),
        "South" => Some(ButtonA),
        "East" => Some(ButtonB),
        "West" => Some(ButtonC),
        "North" => Some(Soft1), // TAB
        _ => None,
    }
}

/// Upgrade only the exact first factory Deck preset, preserving user mappings.
pub fn migrate_steam_deck_gamepad_bindings(
    bindings: &mut std::collections::BTreeMap<String, GuestButton>,
) {
    if !cfg!(feature = "steam-deck") {
        return;
    }
    let mut previous = steam_deck_gamepad_bindings();
    previous.insert("Start".into(), GuestButton::GizPiano1);
    if *bindings == previous {
        *bindings = steam_deck_gamepad_bindings();
    }
}

/// Steam Deck View toggles host fullscreen in both UI and game.
pub fn default_gamepad_fullscreen() -> Option<String> {
    cfg!(feature = "steam-deck").then(|| "Select".to_owned())
}

/// Move the previous factory fullscreen shortcut; preserve other assignments.
pub fn migrate_steam_deck_fullscreen(binding: &mut Option<String>) -> bool {
    if cfg!(feature = "steam-deck") && binding.as_deref() == Some("LeftTrigger") {
        *binding = default_gamepad_fullscreen();
        true
    } else {
        false
    }
}

#[cfg(test)]
mod controller_tests {
    use super::*;
    #[test]
    fn legacy_fullscreen_moves_to_view_without_changing_custom_shortcuts() {
        let mut legacy = Some("LeftTrigger".to_owned());
        let changed = migrate_steam_deck_fullscreen(&mut legacy);
        assert_eq!(changed, cfg!(feature = "steam-deck"));
        assert_eq!(
            legacy.as_deref(),
            Some(if cfg!(feature = "steam-deck") {
                "Select"
            } else {
                "LeftTrigger"
            })
        );
        for binding in [None, Some("West".to_owned()), Some("Select".to_owned())] {
            let mut saved = binding.clone();
            assert!(!migrate_steam_deck_fullscreen(&mut saved));
            assert_eq!(saved, binding);
        }
    }

    #[test]
    fn deck_pocketpc_keys_and_gizmondo_volume_are_separate() {
        use GuestButton::*;
        for (control, button) in [
            ("RightTrigger2", Action),
            ("LeftTrigger2", PocketPcApp2),
            ("RightTrigger", PocketPcApp3),
            ("LeftTrigger", PocketPcApp4),
            ("South", ButtonA),
            ("East", ButtonB),
            ("West", ButtonC),
            ("North", Soft1),
        ] {
            assert_eq!(steam_deck_guest_button(control, false), Some(button));
            assert_eq!(
                steam_deck_guest_button(control, true),
                if control == "LeftTrigger" {
                    Some(GizPiano2)
                } else {
                    None
                }
            );
        }
        for control in ["Start", "Select", "DPadUp", "RightStickLeft"] {
            assert_eq!(steam_deck_guest_button(control, false), None);
            assert_eq!(steam_deck_guest_button(control, true), None);
        }
        assert_eq!(steam_deck_guest_button("North", false).unwrap().vk(), 0x09);
    }

    #[test]
    fn legacy_deck_factory_preset_migrates_without_overwriting_custom_controls() {
        let current = steam_deck_gamepad_bindings();
        let mut previous = current.clone();
        previous.insert("Start".into(), GuestButton::GizPiano1);
        migrate_steam_deck_gamepad_bindings(&mut previous);
        if cfg!(feature = "steam-deck") {
            assert_eq!(previous, current);
        } else {
            assert_eq!(previous.get("Start"), Some(&GuestButton::GizPiano1));
        }
        let mut custom = current.clone();
        custom.insert("Start".into(), GuestButton::ButtonA);
        let saved = custom.clone();
        migrate_steam_deck_gamepad_bindings(&mut custom);
        assert_eq!(custom, saved);
    }

    #[test]
    fn build_defaults_are_scoped_to_the_deck_edition() {
        let config: crate::LauncherConfig = serde_json::from_str("{}").unwrap();
        if cfg!(feature = "steam-deck") {
            assert!(config.gamepad_toggles_fullscreen("Select"));
            assert_eq!(config.gamepad_bindings, steam_deck_gamepad_bindings());
        } else {
            assert_eq!(config.gamepad_fullscreen, None);
            assert_eq!(config.gamepad_bindings, xbox_gamepad_bindings());
        }
    }
    #[test]
    fn xbox_deck_preset_separates_host_fullscreen_from_gizmondo_keys() {
        let mut config = crate::LauncherConfig::default();
        config.gamepad_bindings = steam_deck_gamepad_bindings();
        config.gamepad_fullscreen = Some("LeftTrigger".into());
        assert!(config.gamepad_toggles_fullscreen("LeftTrigger"));
        assert!(!config.gamepad_bindings.contains_key("LeftTrigger"));
        assert_eq!(
            config.gamepad_bindings.get("Start"),
            Some(&GuestButton::GizPiano2)
        );
        assert_eq!(
            config.gamepad_bindings.get("RightTrigger"),
            Some(&GuestButton::GizPiano1)
        );
        assert_eq!(
            config.gamepad_bindings.get("LeftTrigger2"),
            Some(&GuestButton::Soft1)
        );
        assert_eq!(
            config.gamepad_bindings.get("RightTrigger2"),
            Some(&GuestButton::Soft2)
        );
        assert_eq!(
            config.gamepad_bindings.get("South"),
            Some(&GuestButton::Action)
        );
        assert_eq!(
            config.gamepad_bindings.get("North"),
            Some(&GuestButton::ButtonA)
        );
        assert_eq!(
            config.gamepad_bindings.get("East"),
            Some(&GuestButton::ButtonB)
        );
        assert_eq!(
            config.gamepad_bindings.get("West"),
            Some(&GuestButton::ButtonC)
        );
        assert_eq!(
            config.gamepad_bindings.get("DPadUp"),
            Some(&GuestButton::DpadUp)
        );
        assert_eq!(
            config.gamepad_bindings.get("LeftStickUp"),
            Some(&GuestButton::DpadUp)
        );
    }
    #[test]
    fn fullscreen_defaults_preserve_custom_guest_bindings_and_disabled_setting() {
        let mut config: crate::LauncherConfig = serde_json::from_str("{}").unwrap();
        config.gamepad_bindings = steam_deck_gamepad_bindings();
        config.gamepad_fullscreen = Some("LeftTrigger".into());
        assert!(config.gamepad_toggles_fullscreen("LeftTrigger"));
        assert!(!config.gamepad_toggles_fullscreen("Start"));
        config
            .gamepad_bindings
            .insert("LeftTrigger".into(), GuestButton::GizPiano5);
        assert!(!config.gamepad_toggles_fullscreen("LeftTrigger"));
        config.gamepad_bindings.remove("LeftTrigger");
        config.gamepad_fullscreen = None;
        let saved = serde_json::to_string(&config).unwrap();
        let loaded: crate::LauncherConfig = serde_json::from_str(&saved).unwrap();
        assert_eq!(loaded.gamepad_fullscreen, None);
        assert!(!loaded.gamepad_toggles_fullscreen("LeftTrigger"));
        config.gamepad_fullscreen = Some("RightThumb".into());
        let saved = serde_json::to_string(&config).unwrap();
        let loaded: crate::LauncherConfig = serde_json::from_str(&saved).unwrap();
        assert!(loaded.gamepad_toggles_fullscreen("RightThumb"));
    }
    #[test]
    fn old_config_gets_controller_defaults_and_custom_bindings_survive() {
        let old: crate::LauncherConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(
            old.gamepad_bindings.get("South"),
            Some(&GuestButton::Action)
        );
        let mut custom = old;
        custom.gamepad_bindings.clear();
        custom
            .gamepad_bindings
            .insert("East".into(), GuestButton::Action);
        let saved = serde_json::to_string(&custom).unwrap();
        let loaded: crate::LauncherConfig = serde_json::from_str(&saved).unwrap();
        assert_eq!(loaded.gamepad_bindings.len(), 1);
        assert_eq!(
            loaded.gamepad_bindings.get("East"),
            Some(&GuestButton::Action)
        );
        assert_eq!(
            loaded.keybindings.vk_for_key("Z"),
            Some(GuestButton::DpadUp.vk())
        );
        custom.gamepad_bindings.clear();
        let empty: crate::LauncherConfig =
            serde_json::from_str(&serde_json::to_string(&custom).unwrap()).unwrap();
        assert!(empty.gamepad_bindings.is_empty());
    }
}
