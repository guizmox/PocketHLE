//! Process-owned guest CryptoAPI objects.
use serde::{Deserialize, Serialize};
pub use sha1::{Digest, Sha1};
use std::collections::{HashMap, HashSet};

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct CryptoState {
    next: u32,
    pub default_keyset: bool,
    pub providers: HashSet<u32>,
    pub hashes: HashMap<u32, CryptoHash>,
    pub keys: HashMap<u32, CryptoKey>,
    pub public_keys: HashMap<u32, CryptoPublicKey>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CryptoHash {
    pub provider: u32,
    #[serde(with = "digest_state")]
    pub digest: sha1::Sha1,
    pub finished: bool,
    pub value: Option<[u8; 20]>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CryptoKey {
    pub provider: u32,
    pub bytes: [u8; 8],
    pub ecb: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CryptoPublicKey {
    pub provider: u32,
    pub modulus_le: Vec<u8>,
    pub exponent: u32,
}

impl CryptoState {
    pub fn allocate(&mut self) -> Option<u32> {
        if self.providers.len() + self.hashes.len() + self.keys.len() + self.public_keys.len()
            >= 256
        {
            return None;
        }
        self.next = self.next.checked_add(1)?;
        0xe100_0000u32.checked_add(self.next)
    }
}

// Preserve the incremental hash itself, including its partial block. Replaying
// plaintext into a fresh hash would retain an unbounded history of guest data.
mod digest_state {
    use sha1::{
        digest::common::hazmat::{SerializableState, SerializedState},
        Sha1,
    };
    pub fn serialize<S: serde::Serializer>(value: &Sha1, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&SerializableState::serialize(value))
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(decoder: D) -> Result<Sha1, D::Error> {
        let bytes: serde_bytes::ByteBuf = serde::Deserialize::deserialize(decoder)?;
        let state =
            SerializedState::<Sha1>::try_from(bytes.as_ref()).map_err(serde::de::Error::custom)?;
        <Sha1 as SerializableState>::deserialize(&state).map_err(serde::de::Error::custom)
    }
}

impl CryptoState {
    pub(crate) fn validate_snapshot(&self) -> Result<(), String> {
        let mut handles = HashSet::new();
        for &id in self
            .providers
            .iter()
            .chain(self.hashes.keys())
            .chain(self.keys.keys())
            .chain(self.public_keys.keys())
        {
            if id <= 0xe1000000 || id - 0xe1000000 > self.next || !handles.insert(id) {
                return Err("invalid saved CryptoAPI handle".into());
            }
        }
        if handles.len() > 256
            || self
                .hashes
                .values()
                .any(|h| !self.providers.contains(&h.provider))
            || self
                .keys
                .values()
                .any(|k| !self.providers.contains(&k.provider))
            || self.public_keys.values().any(|k| {
                !self.providers.contains(&k.provider)
                    || k.modulus_le.len() > 16384
                    || k.exponent == 0
            })
        {
            return Err("invalid saved CryptoAPI resource".into());
        }
        Ok(())
    }
}
