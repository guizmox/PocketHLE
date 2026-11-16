//! Guest inputs needed to rebuild a programmable GL context.
use super::names::{Kind, ObjectNames};
use super::{error, CallCtx, Result};
use std::{collections::HashMap, sync::Arc};

const MAX_JOURNAL_BYTES: usize = 96 * 1024 * 1024;

#[derive(Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(in crate::gles2) struct Call {
    name: String,
    args: Vec<u32>,
    reads: Vec<(u32, Vec<u8>)>,
    names: Vec<u32>,
    result: u32,
}

impl Call {
    fn size(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.name.len()
            + self.args.len() * 4
            + self.names.len() * 4
            + self
                .reads
                .iter()
                .map(|(_, bytes)| 32 + bytes.len())
                .sum::<usize>()
    }
}

// Calls are immutable. Keep one copy of each exact call and a chronological
// stream of indices so repeated uploads and state changes stay inexpensive.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub(in crate::gles2) struct JournalSnapshot {
    calls: Vec<Arc<Call>>,
    order: Vec<u32>,
}

#[derive(Default)]
pub(super) struct Journal {
    saved: JournalSnapshot,
    call_ids: HashMap<Arc<Call>, u32>,
    bytes: usize,
    failure: Option<String>,
}

impl Journal {
    pub(super) fn begin(&self, name: &str) -> Option<Call> {
        if cfg!(target_os = "android")
            || self.failure.is_some()
            || (name.starts_with("glGet")
                && !matches!(
                    name,
                    "glGetError" | "glGetUniformLocation" | "glGetAttribLocation"
                ))
            || name.starts_with("glIs")
            || matches!(
                name,
                "glFinish" | "glFlush" | "glReadPixels" | "glCheckFramebufferStatus"
            )
        {
            return None;
        }
        Some(Call {
            name: name.to_owned(),
            args: Vec::new(),
            reads: Vec::new(),
            names: Vec::new(),
            result: 0,
        })
    }

    pub(super) fn commit(&mut self, call: Option<Call>, result: u32) {
        let Some(mut call) = call else { return };
        call.result = result;
        self.bytes = self.bytes.saturating_add(std::mem::size_of::<u32>());
        let existing = self.call_ids.get(&call).copied();
        if existing.is_none() {
            self.bytes = self.bytes.saturating_add(dictionary_cost(&call));
        }
        if self.bytes > MAX_JOURNAL_BYTES {
            self.invalidate("OpenGL ES 2.0 snapshot journal exceeded its 96 MiB budget".into());
            return;
        }
        let index = existing.unwrap_or_else(|| {
            let index = self.saved.calls.len() as u32;
            let call = Arc::new(call);
            self.saved.calls.push(call.clone());
            self.call_ids.insert(call, index);
            index
        });
        self.saved.order.push(index);
    }

    pub(super) fn invalidate(&mut self, reason: String) {
        if self.failure.is_none() {
            self.failure = Some(reason);
            self.saved = JournalSnapshot::default();
            self.call_ids = HashMap::new();
        }
    }

    pub(super) fn capture(&self) -> std::result::Result<JournalSnapshot, String> {
        if let Some(reason) = &self.failure {
            return Err(reason.clone());
        }
        Ok(self.saved.clone())
    }

    pub(super) fn replay(
        saved: JournalSnapshot,
        names: &mut ObjectNames,
        mut run: impl FnMut(&str, &mut GlIo<'_, '_>) -> Result<u32>,
    ) -> std::result::Result<Self, String> {
        let mut bytes = saved.order.len().saturating_mul(std::mem::size_of::<u32>());
        let mut call_ids = HashMap::new();
        for (index, call) in saved.calls.iter().enumerate() {
            bytes = bytes.saturating_add(dictionary_cost(call));
            if bytes > MAX_JOURNAL_BYTES {
                return Err("OpenGL ES 2.0 snapshot journal exceeded its 96 MiB budget".into());
            }
            if call_ids.insert(call.clone(), index as u32).is_some() {
                return Err("duplicate OpenGL ES 2.0 snapshot journal entry".into());
            }
        }
        if bytes > MAX_JOURNAL_BYTES
            || saved
                .order
                .iter()
                .any(|&index| index as usize >= saved.calls.len())
        {
            return Err("invalid OpenGL ES 2.0 snapshot journal order".into());
        }

        for &index in &saved.order {
            let call = &saved.calls[index as usize];
            let mut io = GlIo {
                input: Input::Replay(call, 0),
                name: &call.name,
                names,
            };
            let result = run(&call.name, &mut io)
                .map_err(|err| format!("GLES2 snapshot replay failed in {}: {err}", call.name))?;
            if result != call.result
                || !matches!(io.input, Input::Replay(_, reads) if reads == call.reads.len())
            {
                return Err(format!(
                    "GLES2 snapshot replay disagreed with the saved result of {}",
                    call.name
                ));
            }
        }
        Ok(Self {
            saved,
            call_ids,
            bytes,
            failure: None,
        })
    }
}

fn dictionary_cost(call: &Call) -> usize {
    call.size() + 4 * std::mem::size_of::<usize>()
}

enum Input<'a, 'b> {
    Live(&'a mut CallCtx<'b>, Option<&'a mut Call>),
    Replay(&'a Call, usize),
}

pub(super) struct GlIo<'a, 'b> {
    input: Input<'a, 'b>,
    name: &'a str,
    names: &'a mut ObjectNames,
}

impl<'a, 'b> GlIo<'a, 'b> {
    pub(super) fn live(
        ctx: &'a mut CallCtx<'b>,
        call: Option<&'a mut Call>,
        name: &'a str,
        names: &'a mut ObjectNames,
    ) -> Self {
        Self {
            input: Input::Live(ctx, call),
            name,
            names,
        }
    }

    pub(super) fn arg_u32(&mut self, index: u8) -> Result<u32> {
        let raw = match &mut self.input {
            Input::Live(ctx, call) => {
                let value = ctx.arg_u32(index)?;
                if let Some(call) = call {
                    let index = usize::from(index);
                    if index >= call.args.len() {
                        call.args.resize(index + 1, 0);
                    }
                    call.args[index] = value;
                }
                Ok(value)
            }
            Input::Replay(call, _) => call
                .args
                .get(usize::from(index))
                .copied()
                .ok_or_else(|| error("missing GLES2 snapshot argument")),
        }?;
        Ok(if self.names.remapped {
            Kind::argument(self.name, index).map_or(raw, |kind| self.names.host(kind, raw))
        } else {
            raw
        })
    }

    pub(super) fn read_mem(&mut self, address: u32, size: u32) -> Result<Vec<u8>> {
        match &mut self.input {
            Input::Live(ctx, call) => {
                let bytes = ctx.cpu.read_mem(address, size)?;
                if let Some(call) = call {
                    call.reads.push((address, bytes.clone()));
                }
                Ok(bytes)
            }
            Input::Replay(call, next) => {
                let (saved_address, bytes) = call
                    .reads
                    .get(*next)
                    .filter(|(saved_address, bytes)| {
                        *saved_address == address && bytes.len() == size as usize
                    })
                    .ok_or_else(|| error("GLES2 snapshot input does not match the guest read"))?;
                debug_assert_eq!(*saved_address, address);
                *next += 1;
                Ok(bytes.clone())
            }
        }
    }

    pub(super) fn read_mem_into(&mut self, address: u32, out: &mut [u8]) -> Result<()> {
        match &mut self.input {
            Input::Live(ctx, call) => {
                ctx.cpu.read_mem_into(address, out)?;
                if let Some(call) = call {
                    call.reads.push((address, out.to_vec()));
                }
            }
            Input::Replay(..) => out.copy_from_slice(&self.read_mem(address, out.len() as u32)?),
        }
        Ok(())
    }

    pub(super) fn read_u32_le(&mut self, address: u32) -> Result<u32> {
        let mut bytes = [0; 4];
        self.read_mem_into(address, &mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

    pub(super) fn write_mem(&mut self, address: u32, bytes: &[u8]) -> Result<()> {
        if let Input::Live(ctx, _) = &mut self.input {
            ctx.cpu.write_mem(address, bytes)?;
        }
        Ok(())
    }

    pub(super) fn created(&mut self, kind: Kind, host: u32) -> Result<u32> {
        let saved = match self.input {
            Input::Replay(call, _) => Some(call.result),
            _ => None,
        };
        self.names
            .assign(kind, host, saved)
            .ok_or_else(|| error("invalid GLES2 object creation during snapshot restore"))
    }

    pub(super) fn generated(&mut self, host: &[u32], kind: Kind) -> Result<Vec<u32>> {
        let saved = match self.input {
            Input::Replay(call, _) if call.names.len() == host.len() => Some(call.names.as_slice()),
            Input::Replay(..) => return Err(error("invalid GLES2 snapshot object count")),
            _ => None,
        };
        let names = host
            .iter()
            .enumerate()
            .map(|(index, &host)| {
                self.names
                    .assign(kind, host, saved.map(|saved| saved[index]))
                    .ok_or_else(|| error("invalid GLES2 object allocation during snapshot restore"))
            })
            .collect::<Result<Vec<_>>>()?;
        if let Input::Live(_, Some(call)) = &mut self.input {
            call.names.extend_from_slice(&names);
        }
        Ok(names)
    }

    pub(super) fn remove_name(&mut self, kind: Kind, guest: u32) -> u32 {
        let host = self.names.host(kind, guest);
        self.names.remove(kind, guest);
        host
    }

    pub(super) fn remapped(&self) -> bool {
        self.names.remapped
    }

    pub(super) fn guest_name(&self, kind: Kind, host: u32) -> u32 {
        if self.names.remapped {
            self.names.guest(kind, host)
        } else {
            host
        }
    }

    pub(super) fn read_string(&mut self, address: u32, max: u32) -> Result<Vec<u8>> {
        match &mut self.input {
            Input::Live(ctx, call) => {
                let mut output = Vec::new();
                for offset in 0..max {
                    let ptr = address
                        .checked_add(offset)
                        .ok_or_else(|| error("Guest string address overflow"))?;
                    let mut byte = [0];
                    ctx.cpu.read_mem_into(ptr, &mut byte)?;
                    if byte[0] == 0 {
                        if let Some(call) = call {
                            call.reads.push((address, output.clone()));
                        }
                        return Ok(output);
                    }
                    output.push(byte[0]);
                }
                Err(error("Unterminated guest GLES2 string"))
            }
            Input::Replay(call, next) => {
                let (_, bytes) = call
                    .reads
                    .get(*next)
                    .filter(|(saved_address, bytes)| {
                        *saved_address == address && bytes.len() < max as usize
                    })
                    .ok_or_else(|| error("missing GLES2 snapshot string"))?;
                *next += 1;
                Ok(bytes.clone())
            }
        }
    }

    pub(super) fn intern_string(&mut self, text: &str) -> Result<u32> {
        match &mut self.input {
            Input::Live(ctx, _) => crate::gles::intern_string(ctx, text),
            Input::Replay(..) => Err(error("unexpected GLES2 string query in snapshot")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_journal_deduplicates_identical_inputs_without_changing_replay_order() {
        let mut journal = Journal::default();
        for value in [7, 9, 7] {
            let mut call = journal.begin("glBufferSubData").unwrap();
            call.args = vec![value];
            call.reads.push((0x1000, vec![value as u8; 4096]));
            journal.commit(Some(call), value);
        }
        let saved = journal.capture().unwrap();
        assert_eq!(saved.calls.len(), 2);
        assert_eq!(saved.order, [0, 1, 0]);

        let mut expected = [7, 9, 7].into_iter();
        let mut restored = Journal::replay(saved, &mut ObjectNames::default(), |name, io| {
            let value = expected.next().unwrap();
            assert_eq!(name, "glBufferSubData");
            assert_eq!(io.arg_u32(0)?, value);
            assert_eq!(io.read_mem(0x1000, 4096)?, vec![value as u8; 4096]);
            Ok(value)
        })
        .unwrap();
        assert!(expected.next().is_none());

        let mut call = restored.begin("glBufferSubData").unwrap();
        call.args = vec![9];
        call.reads.push((0x1000, vec![9; 4096]));
        restored.commit(Some(call), 9);
        assert_eq!(restored.capture().unwrap().order, [0, 1, 0, 1]);
    }
}
