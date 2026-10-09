//! Colors reads the Gizmondo shell registry before making its first HTTP call.
//! Identity belongs to the persisted device, not to each game/process launch.
use crate::{KernelState, registry::{Registry, RegistryValue}};

pub fn configure(state: &mut KernelState, endpoint: &str, terminal: &str) -> Result<(), String> {
    state.internet.set_colors_endpoint(endpoint).map_err(|_| "Invalid Colors server URL: use http://host:port or https://host:port".to_owned())?;
    configure_identity(&mut state.registry, terminal)
}

fn configure_identity(registry: &mut Registry, terminal: &str) -> Result<(), String> {
    if !terminal.is_empty() && (terminal.len() > 128 || !terminal.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))) {
        return Err("Colors player ID must contain 1..128 letters, digits, '-', '_' or '.'".into());
    }
    let existing = registry.value(r"HKLM\GTShell", "TerminalID");
    let selected = if !terminal.is_empty() {
        Some(terminal.to_owned())
    } else if matches!(existing, Some(RegistryValue::Sz(ref value)) if !value.is_empty()) {
        None
    } else {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?.as_nanos();
        Some(format!("PHLE-{stamp:x}-{:x}", std::process::id()))
    };
    if let Some(value) = selected {
        if !registry.set_value(r"HKLM\GTShell", "TerminalID", RegistryValue::Sz(value)) {
            return Err("Insufficient device RAM for Colors player identity".into());
        }
    }
    if registry.value(r"HKLM\GTShell", "GNS").is_none()
        && !registry.set_value(r"HKLM\GTShell", "GNS", RegistryValue::Sz("us.mygiz.gizmondo.com".into())) {
        return Err("Insufficient device RAM for Gizmondo network settings".into());
    }
    registry.flush().map_err(|e| format!("Cannot save Colors player identity: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_launch_keeps_identity_and_explicit_player_override_is_persisted() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("registry-gizmondo.json");
        let mut registry = Registry::default();
        registry.configure_persistence(&path).unwrap();
        configure_identity(&mut registry, "").unwrap();
        let first = registry.value(r"HKLM\GTShell", "TerminalID");
        configure_identity(&mut registry, "").unwrap();
        assert_eq!(registry.value(r"HKLM\GTShell", "TerminalID"), first);
        configure_identity(&mut registry, "Player-B").unwrap();
        assert_eq!(registry.value(r"HKLM\GTShell", "TerminalID"), Some(RegistryValue::Sz("Player-B".into())));
        let mut reloaded = Registry::default();
        reloaded.configure_persistence(&path).unwrap();
        assert_eq!(reloaded.value(r"HKLM\GTShell", "TerminalID"), Some(RegistryValue::Sz("Player-B".into())));
        assert!(configure_identity(&mut reloaded, "invalid/id").is_err());
        let mut another = Registry::default();
        configure_identity(&mut another, "").unwrap();
        assert_ne!(another.value(r"HKLM\GTShell", "TerminalID"), first);
    }
}
