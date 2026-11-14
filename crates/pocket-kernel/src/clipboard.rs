//! Session clipboard data contains owned bytes, never another process's pointers.
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

pub const NOT_OPEN: u32 = 1418;

#[derive(Debug, Clone)]
pub struct Data {
    pub revision: u64,
    pub bytes: Arc<[u8]>,
}

#[derive(Debug, Default)]
pub struct Client {
    /// Borrowed GetClipboardData handles and transferred SetClipboardData blocks.
    /// Each block belongs to this CPU's heap, even when the data came from a child.
    pub entries: HashMap<u32, (u64, u32)>,
}

#[derive(Default)]
pub struct Clipboard {
    opened: Option<(u32, usize, u32)>,
    owner: Option<(u32, u32)>,
    data: BTreeMap<u32, Data>,
    revision: u64,
}

impl Clipboard {
    fn require_open(&self, pid: u32, thread: usize) -> Result<(), u32> {
        if self.opened.is_some_and(|(p, t, _)| (p, t) == (pid, thread)) {
            Ok(())
        } else {
            Err(NOT_OPEN)
        }
    }

    pub fn open(&mut self, pid: u32, thread: usize, hwnd: u32) -> Result<(), u32> {
        if self.opened.is_some_and(|(p, t, _)| (p, t) != (pid, thread)) {
            return Err(5); // ERROR_ACCESS_DENIED: another thread has it open.
        }
        self.opened = Some((pid, thread, hwnd));
        Ok(())
    }

    pub fn close(&mut self, pid: u32, thread: usize) -> Result<(), u32> {
        self.require_open(pid, thread)?;
        self.opened = None;
        Ok(())
    }

    pub fn empty(&mut self, pid: u32, thread: usize) -> Result<(), u32> {
        self.require_open(pid, thread)?;
        self.owner = self.opened.map(|(pid, _, hwnd)| (pid, hwnd));
        self.data.clear();
        Ok(())
    }

    pub fn set(
        &mut self,
        pid: u32,
        thread: usize,
        format: u32,
        bytes: Vec<u8>,
    ) -> Result<u64, u32> {
        self.require_open(pid, thread)?;
        if !self.owner.is_some_and(|(p, hwnd)| p == pid && hwnd != 0) {
            return Err(NOT_OPEN);
        }
        if format == 0 || format > 0xffff {
            return Err(87);
        }
        self.revision = self.revision.checked_add(1).ok_or(8u32)?;
        self.data.insert(
            format,
            Data {
                revision: self.revision,
                bytes: bytes.into(),
            },
        );
        Ok(self.revision)
    }

    pub fn get(&self, pid: u32, thread: usize, format: u32) -> Result<Option<Data>, u32> {
        self.require_open(pid, thread)?;
        Ok(self.data.get(&format).cloned())
    }

    pub fn next(&self, pid: u32, thread: usize, previous: u32) -> Result<u32, u32> {
        self.require_open(pid, thread)?;
        if previous != 0 && !self.data.contains_key(&previous) {
            return Err(87);
        }
        Ok(self
            .data
            .range((
                std::ops::Bound::Excluded(previous),
                std::ops::Bound::Unbounded,
            ))
            .next()
            .map_or(0, |(&format, _)| format))
    }

    pub fn available(&self, format: u32) -> bool {
        self.data.contains_key(&format)
    }
    pub fn count(&self) -> u32 {
        self.data.len() as u32
    }
    pub fn owner(&self) -> u32 {
        self.owner.map_or(0, |(_, hwnd)| hwnd)
    }
    pub fn revisions(&self) -> BTreeMap<u32, u64> {
        self.data
            .iter()
            .map(|(&format, data)| (format, data.revision))
            .collect()
    }

    pub fn release(&mut self, pid: u32, thread: Option<usize>) {
        if self
            .opened
            .is_some_and(|(p, t, _)| p == pid && thread.is_none_or(|thread| thread == t))
        {
            self.opened = None;
        }
        if thread.is_none() && self.owner.is_some_and(|(p, _)| p == pid) {
            self.owner = None;
        }
        // Contents survive the owner's exit. A later reader receives its own copy.
    }
}
