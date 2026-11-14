//! Device-session state shared by CE services and independently running apps.
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
struct AfsNames {
    next: i32,
    entries: BTreeMap<i32, (u32, String)>,
}

#[derive(Debug, Default)]
struct Power {
    next: u64,
    active: Option<(u32, u64)>,
}

#[derive(Debug, Default)]
struct Device {
    password_active: AtomicBool,
    afs: Mutex<AfsNames>,
    power: Mutex<Power>,
    // An ordinary running app takes only an atomic load at a CPU boundary.
    suspended: AtomicU64,
}

#[derive(Debug, Clone, Default)]
pub struct Context(Arc<Device>);

impl Context {
    /// The stock HLE device has no configured password. This is device state,
    /// not the host account's password or a process-local cached result.
    pub fn password_active(&self) -> bool {
        self.0.password_active.load(Ordering::Acquire)
    }

    pub fn configure_password_active(&self, active: bool) {
        self.0.password_active.store(active, Ordering::Release);
    }

    pub fn register_afs_name(&self, owner: u32, name: String) -> Result<i32, u32> {
        if name.trim().is_empty()
            || name.encode_utf16().count() >= 260
            || matches!(name.as_str(), "." | "..")
            || name
                .chars()
                .any(|c| c == '\\' || c == '/' || c == ':' || c == '\0')
        {
            return Err(87);
        }
        let name = name.to_lowercase();
        let mut names = self.0.afs.lock().unwrap_or_else(|e| e.into_inner());
        if names
            .entries
            .values()
            .any(|(_, registered)| registered == &name)
        {
            return Err(183);
        }
        let next = names.next.checked_add(1).ok_or(8u32)?;
        names.next = next;
        names.entries.insert(next, (owner, name));
        Ok(next)
    }

    pub fn validate_afs_owner(&self, owner: u32, index: i32) -> Result<(), u32> {
        let names = self.0.afs.lock().unwrap_or_else(|e| e.into_inner());
        match names.entries.get(&index) {
            Some((registered_owner, _)) if *registered_owner == owner => Ok(()),
            Some(_) => Err(5),
            None => Err(87),
        }
    }

    pub fn deregister_afs_name(&self, owner: u32, index: i32) -> Result<(), u32> {
        let mut names = self.0.afs.lock().unwrap_or_else(|e| e.into_inner());
        match names.entries.get(&index) {
            Some((registered_owner, _)) if *registered_owner == owner => {
                names.entries.remove(&index);
                Ok(())
            }
            Some(_) => Err(5),
            None => Err(87),
        }
    }

    pub fn suspend(&self, owner: u32) -> u64 {
        let mut power = self.0.power.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, generation)) = power.active {
            return generation;
        }
        power.next = power.next.wrapping_add(1).max(1);
        let generation = power.next;
        power.active = Some((owner, generation));
        self.0.suspended.store(generation, Ordering::Release);
        generation
    }

    pub fn suspended_generation(&self) -> Option<u64> {
        match self.0.suspended.load(Ordering::Acquire) {
            0 => None,
            generation => Some(generation),
        }
    }

    pub fn resume(&self) -> bool {
        self.suspended_generation()
            .is_some_and(|generation| self.resume_generation(generation))
    }

    pub fn resume_generation(&self, generation: u64) -> bool {
        let mut power = self.0.power.lock().unwrap_or_else(|e| e.into_inner());
        if !power.active.is_some_and(|(_, active)| active == generation) {
            return false;
        }
        power.active = None;
        self.0.suspended.store(0, Ordering::Release);
        true
    }

    pub fn release_process(&self, owner: u32) {
        self.0
            .afs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entries
            .retain(|_, (registered_owner, _)| *registered_owner != owner);
        let mut power = self.0.power.lock().unwrap_or_else(|e| e.into_inner());
        if power
            .active
            .is_some_and(|(registered_owner, _)| registered_owner == owner)
        {
            power.active = None;
            self.0.suspended.store(0, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn afs_names_are_case_insensitive_owned_and_released_without_reusing_indices() {
        let device = Context::default();
        let child = device.clone();
        let first = device.register_afs_name(1, "Services".into()).unwrap();
        assert_eq!(child.register_afs_name(2, "SERVICES".into()), Err(183));
        assert_eq!(child.deregister_afs_name(2, first), Err(5));
        let second = child.register_afs_name(2, "Devices".into()).unwrap();
        device.release_process(1);
        assert_eq!(device.validate_afs_owner(1, first), Err(87));
        assert_eq!(child.validate_afs_owner(2, second), Ok(()));
        let replacement = child.register_afs_name(2, "services".into()).unwrap();
        assert!(replacement > second);
        assert_eq!(child.deregister_afs_name(2, replacement), Ok(()));
        assert_eq!(child.deregister_afs_name(2, replacement), Err(87));
        for invalid in ["", " ", ".", "..", "a/b", "a\\b", "a:b", "a\0b"] {
            assert_eq!(device.register_afs_name(1, invalid.into()), Err(87));
        }
    }

    #[test]
    fn suspend_generations_share_wakeup_and_owner_teardown() {
        let device = Context::default();
        let child = device.clone();
        let first = device.suspend(1);
        assert_eq!(child.suspended_generation(), Some(first));
        assert_eq!(child.suspend(2), first);
        child.release_process(2);
        assert_eq!(device.suspended_generation(), Some(first));
        assert!(child.resume());
        assert!(!device.resume());
        let second = child.suspend(2);
        assert_ne!(second, first);
        assert!(!device.resume_generation(first));
        assert_eq!(child.suspended_generation(), Some(second));
        device.release_process(2);
        assert_eq!(child.suspended_generation(), None);
    }
}
