//! Persistent identity of the emulated device, shared by its applications.
use crate::registry::{Registry, RegistryValue};

pub const REGISTRY_KEY: &str = r"HKLM\Software\PocketHLE\DeviceIdentity";
pub const REGISTRY_VALUE: &str = "DeviceID";
const MAX_PART: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceIdentity {
    pub preset: Vec<u8>,
    pub platform: Vec<u8>,
}

impl DeviceIdentity {
    pub fn from_hex(preset: &str, platform: &str) -> Result<Self, String> {
        fn decode(s: &str) -> Result<Vec<u8>, String> {
            if s.len() % 2 != 0
                || s.len() > MAX_PART * 2
                || !s.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err("Device ID must contain pairs of hexadecimal digits (maximum 4096 bytes per part)".into());
            }
            (0..s.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
                .collect()
        }
        let id = Self {
            preset: decode(preset)?,
            platform: decode(platform)?,
        };
        id.validate()?;
        Ok(id)
    }

    fn validate(&self) -> Result<(), String> {
        if self.preset.len() > MAX_PART
            || self.platform.len() > MAX_PART
            || (self.preset.is_empty() && self.platform.is_empty())
        {
            Err("Device identity requires at least one nonempty identifier".into())
        } else {
            Ok(())
        }
    }

    /// DEVICE_ID: five DWORDs followed by opaque identifier bytes.
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let preset_len = self.preset.len() as u32;
        let platform_len = self.platform.len() as u32;
        let platform_offset = (20 + preset_len + 3) & !3;
        let mut data: Vec<u8> = [
            platform_offset + platform_len,
            20,
            preset_len,
            platform_offset,
            platform_len,
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
        data.extend_from_slice(&self.preset);
        data.resize(platform_offset as usize, 0);
        data.extend_from_slice(&self.platform);
        Ok(data)
    }

    fn decode(data: &[u8]) -> Result<Self, String> {
        if data.len() < 20 {
            return Err("Invalid persisted DEVICE_ID header".into());
        }
        let word = |i| u32::from_le_bytes(data[i..i + 4].try_into().unwrap()) as usize;
        let part = |offset: usize, length: usize| -> Result<Vec<u8>, String> {
            if offset < 20 || length > MAX_PART {
                return Err("Invalid persisted DEVICE_ID part".into());
            }
            data.get(
                offset
                    ..offset
                        .checked_add(length)
                        .ok_or("Invalid DEVICE_ID length")?,
            )
            .map(|b| b.to_vec())
            .ok_or_else(|| "Invalid persisted DEVICE_ID bounds".into())
        };
        if word(0) != data.len() {
            return Err("Invalid persisted DEVICE_ID size".into());
        }
        let id = Self {
            preset: part(word(4), word(8))?,
            platform: part(word(12), word(16))?,
        };
        id.validate()?;
        Ok(id)
    }

    pub fn save(&self, registry: &mut Registry) -> Result<(), String> {
        let encoded = self.encode()?;
        if !registry.set_value(REGISTRY_KEY, REGISTRY_VALUE, RegistryValue::Binary(encoded)) {
            return Err("Insufficient registry storage for device identity".into());
        }
        registry
            .flush()
            .map_err(|e| format!("Cannot persist device identity: {e}"))
    }

    pub fn configure(
        registry: &mut Registry,
        preset: &str,
        platform: &str,
    ) -> Result<Self, String> {
        // Validate overrides before generating or persisting anything.
        Self::from_hex(preset, if platform.is_empty() { "00" } else { platform })?;
        let previous = Self::load_or_create(registry)?;
        let platform = if platform.is_empty() {
            Self::hex(&previous.platform)
        } else {
            platform.into()
        };
        let id = Self::from_hex(preset, &platform)?;
        if id != previous {
            id.save(registry)?;
        }
        Ok(id)
    }

    pub fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02X}")).collect()
    }

    pub fn load_or_create(registry: &mut Registry) -> Result<Self, String> {
        if let Some(value) = registry.value(REGISTRY_KEY, REGISTRY_VALUE) {
            return match value {
                RegistryValue::Binary(data) => Self::decode(&data),
                _ => Err("Persisted device identity is not REG_BINARY".into()),
            };
        }
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let unique = stamp
            ^ ((std::process::id() as u128) << 64)
            ^ ((SEQUENCE.fetch_add(1, Ordering::Relaxed) as u128) << 96);
        let id = Self {
            preset: b"PocketHLE-WinCE".to_vec(),
            platform: unique.to_le_bytes().to_vec(),
        };
        id.save(registry)?;
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_identity_is_stable_across_registry_reload_and_supports_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("device.json");
        let mut registry = Registry::default();
        registry.configure_persistence(&path).unwrap();
        let initial = DeviceIdentity::load_or_create(&mut registry).unwrap();
        assert_eq!(
            DeviceIdentity::load_or_create(&mut registry).unwrap(),
            initial
        );
        let mut reopened = Registry::default();
        reopened.configure_persistence(&path).unwrap();
        assert_eq!(
            DeviceIdentity::load_or_create(&mut reopened).unwrap(),
            initial
        );
        let custom =
            DeviceIdentity::from_hex("506F636B6574484C45", "00112233445566778899AABBCCDDEEFF")
                .unwrap();
        custom.save(&mut reopened).unwrap();
        let mut again = Registry::default();
        again.configure_persistence(&path).unwrap();
        assert_eq!(DeviceIdentity::load_or_create(&mut again).unwrap(), custom);
        assert!(DeviceIdentity::from_hex("", "").is_err());
        assert!(DeviceIdentity::from_hex("a", "00").is_err());
        assert!(DeviceIdentity::from_hex("zz", "00").is_err());
    }

    #[test]
    fn gizmondo_layout_and_profile_configuration_preserve_generated_platform() {
        let mut registry = Registry::default();
        let preset = "470069007A006D006F006E0064006F000000";
        let a = DeviceIdentity::configure(&mut registry, preset, "").unwrap();
        let b = DeviceIdentity::configure(&mut registry, preset, "").unwrap();
        assert_eq!(a, b);
        let bytes = a.encode().unwrap();
        assert_eq!(bytes.len(), 56);
        assert_eq!(u32::from_le_bytes(bytes[12..16].try_into().unwrap()), 40);
        assert_eq!(&bytes[38..40], &[0, 0]);
        assert_eq!(DeviceIdentity::decode(&bytes).unwrap(), a);
        assert!(DeviceIdentity::configure(&mut registry, "zz", "00").is_err());
        assert_eq!(DeviceIdentity::load_or_create(&mut registry).unwrap(), a);
        let custom =
            DeviceIdentity::configure(&mut registry, preset, "00112233445566778899AABBCCDDEEFF")
                .unwrap();
        assert_ne!(custom.platform, a.platform);
    }

    #[test]
    fn invalid_stored_identity_is_not_silently_replaced() {
        let mut registry = Registry::default();
        registry.set_value(
            REGISTRY_KEY,
            REGISTRY_VALUE,
            RegistryValue::Binary(vec![0; 20]),
        );
        let before = registry.value(REGISTRY_KEY, REGISTRY_VALUE);
        assert!(DeviceIdentity::load_or_create(&mut registry).is_err());
        assert_eq!(registry.value(REGISTRY_KEY, REGISTRY_VALUE), before);
    }
}
