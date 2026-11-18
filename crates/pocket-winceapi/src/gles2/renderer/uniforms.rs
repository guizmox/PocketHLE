//! A native relink during restore may assign different uniform locations.
//! Keep the complete per-program bijection only when identity is insufficient.
use std::collections::HashMap;

use super::{error, Result};

#[derive(Default)]
pub(super) struct UniformLocations {
    guest_to_host: HashMap<i32, i32>,
    host_to_guest: HashMap<i32, i32>,
}

impl UniformLocations {
    pub(super) fn restored(pairs: impl IntoIterator<Item = (i32, i32)>) -> Result<Option<Self>> {
        let mut map = Self::default();
        let mut changed = false;
        for (guest, host) in pairs {
            if guest < 0
                || host < 0
                || map.guest_to_host.insert(guest, host).is_some()
                || map.host_to_guest.insert(host, guest).is_some()
            {
                return Err(error("invalid saved programmable uniform identity"));
            }
            changed |= guest != host;
        }
        Ok(changed.then_some(map))
    }

    pub(super) fn host(&self, guest: i32) -> Option<i32> {
        if guest == -1 {
            // Setters must still reach GL: -1 is ignored only with otherwise
            // valid state/arguments, and queries must reject it.
            Some(-1)
        } else {
            self.guest_to_host.get(&guest).copied()
        }
    }

    pub(super) fn guest(&self, host: i32) -> Option<i32> {
        if host == -1 {
            Some(-1)
        } else {
            self.host_to_guest.get(&host).copied()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_needs_no_runtime_translation() -> Result<()> {
        assert!(UniformLocations::restored([])?.is_none());
        assert!(UniformLocations::restored([(0, 0), (7, 7)])?.is_none());
        Ok(())
    }

    #[test]
    fn permutation_preserves_guest_names_and_rejects_native_collisions() -> Result<()> {
        let map = UniformLocations::restored([(0, 1), (1, 5), (3, 0)])?.unwrap();
        assert_eq!(map.host(0), Some(1));
        assert_eq!(map.guest(0), Some(3));
        assert_eq!(map.host(5), None);
        assert_eq!(map.guest(8), None);
        assert_eq!(map.host(-1), Some(-1));
        assert_eq!(map.host(-2), None);
        assert_eq!(map.guest(-1), Some(-1));
        for pairs in [
            vec![(-1, 0)],
            vec![(0, -1)],
            vec![(0, 0), (0, 1)],
            vec![(0, 1), (2, 1)],
        ] {
            assert!(UniformLocations::restored(pairs).is_err());
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod native_tests;
