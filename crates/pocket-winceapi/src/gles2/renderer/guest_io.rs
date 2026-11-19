//! Guest memory access and stable object names shared by both native GL APIs.
use super::names::{Kind, ObjectNames};
use super::{error, CallCtx, Result};

pub(super) struct GlIo<'a, 'b> {
    ctx: &'a mut CallCtx<'b>,
    name: &'a str,
    names: &'a mut ObjectNames,
}
impl<'a, 'b> GlIo<'a, 'b> {
    pub(super) fn live(
        ctx: &'a mut CallCtx<'b>,
        name: &'a str,
        names: &'a mut ObjectNames,
    ) -> Self {
        Self { ctx, name, names }
    }
    pub(super) fn arg_u32(&mut self, index: u8) -> Result<u32> {
        let raw = self.ctx.arg_u32(index)?;
        Ok(if self.names.remapped {
            Kind::argument(self.name, index).map_or(raw, |kind| self.names.host(kind, raw))
        } else {
            raw
        })
    }
    pub(super) fn read_mem(&mut self, address: u32, size: u32) -> Result<Vec<u8>> {
        Ok(self.ctx.cpu.read_mem(address, size)?)
    }
    pub(super) fn read_mem_into(&mut self, address: u32, out: &mut [u8]) -> Result<()> {
        Ok(self.ctx.cpu.read_mem_into(address, out)?)
    }
    pub(super) fn read_u32_le(&mut self, address: u32) -> Result<u32> {
        let mut bytes = [0; 4];
        self.read_mem_into(address, &mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }
    pub(super) fn write_mem(&mut self, address: u32, bytes: &[u8]) -> Result<()> {
        Ok(self.ctx.cpu.write_mem(address, bytes)?)
    }
    pub(super) fn created(&mut self, kind: Kind, host: u32) -> Result<u32> {
        self.names
            .assign(kind, host, None)
            .ok_or_else(|| error("invalid GLES2 object allocation"))
    }
    pub(super) fn generated(&mut self, host: &[u32], kind: Kind) -> Result<Vec<u32>> {
        host.iter().map(|&host| self.created(kind, host)).collect()
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
        let mut output = Vec::new();
        for offset in 0..max {
            let ptr = address
                .checked_add(offset)
                .ok_or_else(|| error("Guest string address overflow"))?;
            let mut byte = [0];
            self.ctx.cpu.read_mem_into(ptr, &mut byte)?;
            if byte[0] == 0 {
                return Ok(output);
            }
            output.push(byte[0]);
        }
        Err(error("Unterminated guest GLES2 string"))
    }
    pub(super) fn intern_string(&mut self, text: &str) -> Result<u32> {
        crate::gles::intern_string(self.ctx, text)
    }
}
