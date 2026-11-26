//! Guest memory access and stable object names shared by both native GL APIs.
use super::names::{Kind, ObjectNames};
use super::{error, CallCtx, Result};

pub(super) struct GlIo<'a, 'b> {
    ctx: &'a mut CallCtx<'b>,
    name: &'a str,
    names: &'a mut ObjectNames,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocket_cpu::{regs::ArmReg, Cpu};
    use pocket_kernel::Thunk;
    use pocket_pe::ImportBinding;

    #[test]
    fn vertex_pointer_arguments_remain_values_after_object_name_remapping() {
        let (mut cpu, mut kernel, _) = crate::bluetooth::tests::setup();
        let thunk = Thunk {
            thunk_va: 0,
            iat_va: 0,
            dll: "libglesv2.dll".into(),
            binding: ImportBinding::Name("glVertexAttribPointer".into()),
            friendly_name: Some("glVertexAttribPointer".into()),
        };
        let expected = [2, 3, 0x1406, 0xff, 24, 0x80000020];
        for (index, &value) in expected[..4].iter().enumerate() {
            cpu.write_arg(index as u8, value).unwrap();
        }
        cpu.write_mem(0x4000, &24u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x4004, &0x80000020u32.to_le_bytes()).unwrap();
        let mut names = ObjectNames::default();
        names.assign(Kind::Buffer, 101, Some(2)).unwrap();
        assert!(names.remapped);
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        assert_eq!(
            GlIo::live(&mut ctx, "glVertexAttribPointer", &mut names)
                .args6()
                .unwrap(),
            expected
        );
        // No persistent per-call cache: new declarations see changed values.
        ctx.cpu.write_reg(ArmReg::R0, 1).unwrap();
        ctx.cpu.write_mem(0x4004, &64u32.to_le_bytes()).unwrap();
        let fresh = GlIo::live(&mut ctx, "glVertexAttribPointer", &mut names)
            .args6()
            .unwrap();
        assert_eq!((fresh[0], fresh[5]), (1, 64));
    }
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
    // Electopia repeatedly declares six-argument vertex attributes. Group the
    // register reads and the two stack words without changing name remapping
    // or speculatively touching a seventh argument.
    pub(super) fn args6(&mut self) -> Result<[u32; 6]> {
        let mut args = self.ctx.cpu.read_args6()?;
        if self.names.remapped {
            for (index, value) in args.iter_mut().enumerate() {
                if let Some(kind) = Kind::argument(self.name, index as u8) {
                    *value = self.names.host(kind, *value);
                }
            }
        }
        Ok(args)
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
