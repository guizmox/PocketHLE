//! Per-process HGLOBAL streams. Clones share storage and keep their own cursors.
use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
pub struct Stream {
    pub vtable: u32,
    pub refs: u32,
    pub backing: u64,
    pub position: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct Backing {
    pub handle: u32,
    pub capacity: u32,
    pub length: u32,
    pub streams: u32,
    pub delete_on_release: bool,
}

#[derive(Debug, Default)]
pub struct Streams {
    pub next_id: u64,
    pub objects: HashMap<u32, Stream>,
    pub backings: HashMap<u64, Backing>,
}
