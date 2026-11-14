//! CE sound schemes are positional REG_MULTI_SZ arrays, including empty slots.
//! Explorer's default scheme deliberately silences menus and window transitions.
use crate::{registry::Registry, registry::RegistryValue, GuestCallFrame};

// Private mixer streams, never exposed as guest HWAVEOUT handles. Stopping a
// PlaySound clip must not purge ceplayer's PCM or a game's independent mixer.
pub const PLAY_SOUND_STREAM: u32 = 0xffff_fffe;
pub const EVENT_SOUND_STREAM: u32 = 0xffff_fffd;
pub const EVENT_KEY: &str = r"HKLM\Snd\Event";
pub const VOLUME_KEY: &str = r"HKCU\ControlPanel\Volume";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum SystemEvent {
    Asterisk,
    Close,
    Critical,
    Default,
    EmptyRecycleBin,
    Exclamation,
    DownloadBegin,
    DownloadEnd,
    DownloadInterrupted,
    MenuPopup,
    MenuSelect,
    Open,
    Question,
    NetworkBegin,
    NetworkEnd,
    NetworkInterrupted,
    Startup,
    Maximize,
    Minimize,
    RecordingStart,
    RecordingEnd,
}

const ALL_SOUNDS: [&str; 21] = [
    "asterisk", "close", "critical", "default", "empty", "exclam", "infbeg", "infend", "infintr",
    "menupop", "menusel", "openprog", "question", "infbeg", "infend", "infintr", "startup",
    "windmax", "windmin", "recstart", "recend",
];
const DEFAULT_SOUNDS: [&str; 21] = [
    "asterisk", "", "critical", "default", "", "exclam", "infbeg", "infend", "infintr", "", "", "",
    "question", "infbeg", "infend", "infintr", "startup", "", "", "", "",
];

impl SystemEvent {
    pub fn from_alias(alias: &str) -> Option<Self> {
        Some(match alias.to_ascii_lowercase().as_str() {
            "systemasterisk" | "asterisk" => Self::Asterisk,
            "systemexit" | "systemclose" | "close" => Self::Close,
            "systemhand" | "critical" => Self::Critical,
            "systemdefault" | "default" => Self::Default,
            "emptyrecyclebin" | "empty" => Self::EmptyRecycleBin,
            "systemexclamation" | "exclamation" | "exclam" => Self::Exclamation,
            "downloadbegin" => Self::DownloadBegin,
            "downloadend" => Self::DownloadEnd,
            "downloadinterrupted" => Self::DownloadInterrupted,
            "menupopup" | "menupop" => Self::MenuPopup,
            "menuselect" | "menusel" => Self::MenuSelect,
            "open" | "openprog" => Self::Open,
            "systemquestion" | "question" => Self::Question,
            "networkbegin" | "infbeg" => Self::NetworkBegin,
            "networkend" | "infend" => Self::NetworkEnd,
            "networkinterrupted" | "infintr" => Self::NetworkInterrupted,
            "systemstart" | "systemwelcome" | "startup" => Self::Startup,
            "maximize" | "windmax" => Self::Maximize,
            "minimize" | "windmin" => Self::Minimize,
            "recordingstart" | "recstart" => Self::RecordingStart,
            "recordingend" | "recend" => Self::RecordingEnd,
            _ => return None,
        })
    }

    pub fn from_alias_id(id: u32) -> Option<Self> {
        // SDK mmsystem.h: sndAlias('S', ch), used with SND_ALIAS_ID.
        Some(match id {
            0x2a53 => Self::Asterisk,
            0x3f53 => Self::Question,
            0x4853 => Self::Critical,
            0x4553 => Self::Close,
            0x5353 | 0x5753 => Self::Startup,
            0x2153 => Self::Exclamation,
            0x4453 => Self::Default,
            _ => return None,
        })
    }

    pub fn from_message_beep(kind: u32) -> Option<Self> {
        Some(match kind {
            0 | u32::MAX => Self::Default,
            0x10 => Self::Critical,
            0x20 => Self::Question,
            0x30 => Self::Exclamation,
            0x40 => Self::Asterisk,
            _ => return None,
        })
    }

    fn alias(self) -> &'static str {
        match self {
            Self::Asterisk => "SystemAsterisk",
            Self::Close => "SystemExit",
            Self::Critical => "SystemHand",
            Self::Default => "SystemDefault",
            Self::EmptyRecycleBin => "EmptyRecycleBin",
            Self::Exclamation => "SystemExclamation",
            Self::DownloadBegin => "DownloadBegin",
            Self::DownloadEnd => "DownloadEnd",
            Self::DownloadInterrupted => "DownloadInterrupted",
            Self::MenuPopup => "MenuPopup",
            Self::MenuSelect => "MenuSelect",
            Self::Open => "Open",
            Self::Question => "SystemQuestion",
            Self::NetworkBegin => "NetworkBegin",
            Self::NetworkEnd => "NetworkEnd",
            Self::NetworkInterrupted => "NetworkInterrupted",
            Self::Startup => "SystemStart",
            Self::Maximize => "Maximize",
            Self::Minimize => "Minimize",
            Self::RecordingStart => "RecordingStart",
            Self::RecordingEnd => "RecordingEnd",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SoundCategory {
    #[default]
    Application,
    Event,
    Notification,
}
impl SoundCategory {
    pub fn volume(self, registry: &Registry) -> u32 {
        let mask = match self {
            Self::Application => 2,
            Self::Event => 4,
            Self::Notification => 1,
        };
        let mute = match registry.value(VOLUME_KEY, "Mute") {
            Some(RegistryValue::Dword(value)) => value,
            _ => 7,
        };
        if mute & mask == 0 {
            return 0;
        }
        match registry.value(VOLUME_KEY, "Volume") {
            Some(RegistryValue::Dword(value)) => value,
            _ => u32::MAX,
        }
    }
}

#[derive(Debug)]
pub struct SoundWait {
    pub thread: usize,
    pub thunk: u32,
    pub frame: GuestCallFrame,
    pub generation: u64,
    pub samples: u64,
}
#[derive(Debug, Default)]
pub struct SoundState {
    pub startup_pending: bool,
    pub generation: u64,
    pub category: SoundCategory,
    pub waits: Vec<SoundWait>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SoundSelection {
    File(String),
    Silent,
    Missing,
}
fn sound_path(name: &str) -> SoundSelection {
    if name.is_empty() {
        return SoundSelection::Silent;
    }
    let mut path = if name.starts_with(['\\', '/']) {
        name.to_owned()
    } else {
        format!(r"\Windows\{name}")
    };
    if !path.rsplit(['\\', '/']).next().unwrap_or("").contains('.') {
        path.push_str(".wav");
    }
    SoundSelection::File(path)
}

pub fn current_scheme(registry: &Registry) -> String {
    match registry.value(EVENT_KEY, ".Scheme") {
        Some(RegistryValue::Sz(name)) if !name.is_empty() => name,
        _ => ".DefaultSounds".into(),
    }
}
pub fn schemes(registry: &Registry) -> Vec<String> {
    let mut names: Vec<String> = [".DefaultSounds", ".AllSounds", ".NoSounds"]
        .into_iter()
        .map(String::from)
        .collect();
    for (name, value) in registry.values(EVENT_KEY) {
        if matches!(value, RegistryValue::Raw { kind: 7, .. })
            && !names.iter().any(|n| n.eq_ignore_ascii_case(&name))
        {
            names.push(name);
        }
    }
    let current = current_scheme(registry);
    if !names.iter().any(|n| n.eq_ignore_ascii_case(&current)) {
        names.push(current);
    }
    names
}

fn override_sound(registry: &Registry, alias: &str) -> Option<SoundSelection> {
    for (key, name) in [
        (format!(r"HKCU\ControlPanel\Sounds\{alias}"), "Sound"),
        (
            format!(r"HKCU\AppEvents\Schemes\Apps\.Default\{alias}\.Current"),
            "",
        ),
    ] {
        if let Some(RegistryValue::Sz(sound)) = registry.value(&key, name) {
            return Some(sound_path(&sound));
        }
    }
    match registry.value(EVENT_KEY, alias) {
        Some(RegistryValue::Sz(sound)) => Some(sound_path(&sound)),
        _ => None,
    }
}

pub fn event_sound(registry: &Registry, event: SystemEvent) -> SoundSelection {
    let scheme = current_scheme(registry);
    let index = event as usize;
    if scheme.eq_ignore_ascii_case(".NoSounds")
        || matches!(registry.value(EVENT_KEY, "EventMask"), Some(RegistryValue::Dword(mask)) if mask & (1 << index) == 0)
    {
        return SoundSelection::Silent;
    }
    if let Some(sound) = override_sound(registry, event.alias()) {
        return sound;
    }
    if let Some(RegistryValue::Raw { kind: 7, bytes }) = registry.value(EVENT_KEY, &scheme) {
        if bytes.len() % 2 != 0 {
            return SoundSelection::Missing;
        }
        let units: Vec<_> = bytes
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        // Double NULs inside a CE scheme are silent slots, not its end.
        return units
            .split(|u| *u == 0)
            .nth(index)
            .map(|name| sound_path(&String::from_utf16_lossy(name)))
            .unwrap_or(SoundSelection::Missing);
    }
    let names = if scheme.eq_ignore_ascii_case(".DefaultSounds") {
        &DEFAULT_SOUNDS
    } else if scheme.eq_ignore_ascii_case(".AllSounds") {
        &ALL_SOUNDS
    } else {
        return SoundSelection::Missing;
    };
    sound_path(names[index])
}

pub fn alias_sound(registry: &Registry, alias: &str) -> SoundSelection {
    if current_scheme(registry).eq_ignore_ascii_case(".NoSounds") {
        return SoundSelection::Silent;
    }
    if let Some(event) = SystemEvent::from_alias(alias) {
        if matches!(registry.value(EVENT_KEY, "EventMask"), Some(RegistryValue::Dword(mask)) if mask & (1 << event as usize) == 0)
        {
            return SoundSelection::Silent;
        }
        // An explicit CE alias spelling may have its own user override.
        if let Some(sound) = override_sound(registry, alias) {
            return sound;
        }
        return event_sound(registry, event);
    }
    override_sound(registry, alias).unwrap_or(SoundSelection::Missing)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ce_multi_strings_preserve_silent_slots_and_later_startup_and_menu_files() {
        let mut registry = Registry::with_device_defaults();
        let bytes = DEFAULT_SOUNDS
            .iter()
            .flat_map(|name| name.encode_utf16().chain(Some(0)))
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        registry.set_value(
            EVENT_KEY,
            ".DefaultSounds",
            RegistryValue::Raw { kind: 7, bytes },
        );
        assert_eq!(
            event_sound(&registry, SystemEvent::Close),
            SoundSelection::Silent
        );
        assert_eq!(
            event_sound(&registry, SystemEvent::Critical),
            SoundSelection::File(r"\Windows\critical.wav".into())
        );
        assert_eq!(
            event_sound(&registry, SystemEvent::MenuPopup),
            SoundSelection::Silent
        );
        assert_eq!(
            event_sound(&registry, SystemEvent::Startup),
            SoundSelection::File(r"\Windows\startup.wav".into())
        );
        registry.set_value(EVENT_KEY, ".Scheme", RegistryValue::Sz(".AllSounds".into()));
        assert_eq!(
            event_sound(&registry, SystemEvent::MenuPopup),
            SoundSelection::File(r"\Windows\menupop.wav".into())
        );
        registry.set_value(EVENT_KEY, "EventMask", RegistryValue::Dword(0x1ffff));
        assert_eq!(
            event_sound(&registry, SystemEvent::Maximize),
            SoundSelection::Silent
        );
    }
    #[test]
    fn custom_aliases_and_explicit_silence_do_not_fall_back_to_another_sound() {
        let mut registry = Registry::with_device_defaults();
        registry.set_value(
            r"HKCU\ControlPanel\Sounds\SystemQuestion",
            "Sound",
            RegistryValue::Sz(r"\My Documents\alert.wav".into()),
        );
        assert_eq!(
            alias_sound(&registry, "systemquestion"),
            SoundSelection::File(r"\My Documents\alert.wav".into())
        );
        registry.set_value(
            r"HKCU\ControlPanel\Sounds\SystemQuestion",
            "Sound",
            RegistryValue::Sz(String::new()),
        );
        assert_eq!(
            alias_sound(&registry, "SystemQuestion"),
            SoundSelection::Silent
        );
        assert_eq!(
            alias_sound(&registry, "UnknownEvent"),
            SoundSelection::Missing
        );
        registry.set_value(EVENT_KEY, ".Scheme", RegistryValue::Sz(".NoSounds".into()));
        assert_eq!(
            event_sound(&registry, SystemEvent::Startup),
            SoundSelection::Silent
        );
        assert_eq!(
            alias_sound(&registry, "UnknownEvent"),
            SoundSelection::Silent
        );
    }
}
