//! Owned metadata for native CE shell API providers. Guest method addresses
//! always retain their process identity; registration does not execute them.
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ApiSet {
    pub owner: u32,
    pub name: [u8; 4],
    pub methods: Vec<u32>,
    pub signatures: Vec<u64>,
    pub id: Option<u32>,
}

#[derive(Default)]
pub(crate) struct Registry {
    sets: HashMap<u32, ApiSet>,
    ids: HashMap<u32, u32>,
}
impl Registry {
    pub fn insert(&mut self, handle: u32, set: ApiSet) {
        self.sets.insert(handle, set);
    }
    pub fn register(&mut self, owner: u32, handle: u32, id: u32) -> Result<(), u32> {
        let set = self
            .sets
            .get(&handle)
            .filter(|s| s.owner == owner)
            .ok_or(6u32)?;
        if id & 0x8000_0000 != 0 {
            // Handle-based servers need CreateAPIHandle and argument marshalling;
            // they cannot be advertised as the shell's non-handle provider.
            return Err(50);
        }
        if id == 0 || id >= 128 {
            return Err(87);
        }
        if set.id == Some(id) {
            return Ok(());
        }
        if set.id.is_some() {
            return Err(87);
        }
        if self.ids.contains_key(&id)
            || self
                .sets
                .values()
                .any(|s| s.id.is_some() && s.name == set.name)
        {
            return Err(183);
        }
        self.sets.get_mut(&handle).unwrap().id = Some(id);
        self.ids.insert(id, handle);
        Ok(())
    }
    pub fn query(&self, name: [u8; 4]) -> Option<u32> {
        self.sets
            .values()
            .find(|s| s.name == name && s.id.is_some())
            .and_then(|s| s.id)
    }
    pub fn provider(&self, id: u32) -> Option<ApiSet> {
        self.sets.get(self.ids.get(&id)?).cloned()
    }
    pub fn ready(&self, id: u32) -> bool {
        self.ids.contains_key(&id)
    }
    pub fn remove(&mut self, handle: u32) {
        if let Some(set) = self.sets.remove(&handle) {
            if let Some(id) = set.id {
                self.ids.remove(&id);
            }
        }
    }
    pub fn release(&mut self, owner: u32) {
        let keys: Vec<_> = self
            .sets
            .iter()
            .filter_map(|(&k, s)| (s.owner == owner).then_some(k))
            .collect();
        for key in keys {
            self.remove(key);
        }
    }
}
