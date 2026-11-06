//! Process-owned guest CryptoAPI objects.
pub use sha1::{Digest, Sha1};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct CryptoState {
    next: u32,
    pub default_keyset: bool,
    pub providers: HashSet<u32>,
    pub hashes: HashMap<u32, CryptoHash>,
    pub keys: HashMap<u32, CryptoKey>,
    pub public_keys: HashMap<u32, CryptoPublicKey>,
}

pub struct CryptoHash {
    pub provider: u32,
    pub digest: sha1::Sha1,
    pub finished: bool,
    pub value: Option<[u8; 20]>,
}

pub struct CryptoKey {
    pub provider: u32,
    pub bytes: [u8; 8],
    pub ecb: bool,
}

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
