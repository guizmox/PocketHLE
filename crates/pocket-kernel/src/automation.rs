//! Guest-owned Automation interfaces over immutable type-library metadata.
use pocket_pe::typelib::Library;
use std::{collections::HashMap, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Library,
    TypeInfo { parent: u32, index: usize },
}

#[derive(Debug, Clone)]
pub struct Object {
    pub vtable: u32,
    pub refs: u32,
    pub key: String,
    pub metadata: Arc<Library>,
    pub kind: Kind,
}

#[derive(Debug)]
pub struct Descriptor {
    pub owner: u32,
    pub allocations: Vec<u32>,
}

#[derive(Debug, Default)]
pub struct State {
    pub parsed: HashMap<String, Arc<Library>>,
    pub objects: HashMap<u32, Object>,
    pub descriptors: HashMap<u32, Descriptor>,
}
