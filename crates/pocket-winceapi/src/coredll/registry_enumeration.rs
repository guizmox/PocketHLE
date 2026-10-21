//! Registry enumeration uses real stored names/data, not fabricated success.
use super::*;
fn code(value: u32) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(value))
}
fn inout(ctx: &CallCtx<'_>, ptr: u32) -> bool {
    ptr != 0
        && ctx
            .cpu
            .check_guest_access(ptr, 4, Prot::READ | Prot::WRITE)
            .is_ok()
}
pub(super) fn enum_value(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let key = ctx.arg_u32(0)?;
    let index = ctx.arg_u32(1)?;
    let name = ctx.arg_u32(2)?;
    let name_size = ctx.arg_u32(3)?;
    let reserved = ctx.arg_u32(4)?;
    let kind = ctx.arg_u32(5)?;
    let data = ctx.arg_u32(6)?;
    let data_size = ctx.arg_u32(7)?;
    let Some(path) = ctx.kernel.registry.path_for(key) else {
        return code(6);
    };
    if reserved != 0
        || name == 0
        || !inout(ctx, name_size)
        || (data != 0 && data_size == 0)
        || (data_size != 0 && !inout(ctx, data_size))
        || (kind != 0 && !vfs_probe_output(ctx, kind, 4))
    {
        return code(87);
    }
    let values = ctx.kernel.registry.values(&path);
    let Some((value_name, value)) = values.get(index as usize) else {
        return code(259);
    };
    let chars: Vec<u16> = value_name.encode_utf16().collect();
    let bytes = value.to_bytes();
    let fits_name = ctx.cpu.read_u32_le(name_size)? > chars.len() as u32;
    let fits_data = data == 0 || ctx.cpu.read_u32_le(data_size)? as usize >= bytes.len();
    let name_bytes: Vec<_> = chars
        .iter()
        .copied()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    if (fits_name && !vfs_probe_output(ctx, name, name_bytes.len() as u32))
        || (fits_name && fits_data && data != 0 && !vfs_probe_output(ctx, data, bytes.len() as u32))
    {
        return code(87);
    }
    ctx.cpu.write_mem(
        name_size,
        &((chars.len() + usize::from(!fits_name)) as u32).to_le_bytes(),
    )?;
    if data_size != 0 {
        ctx.cpu
            .write_mem(data_size, &(bytes.len() as u32).to_le_bytes())?;
    }
    if kind != 0 {
        ctx.cpu.write_mem(kind, &value.type_code().to_le_bytes())?;
    }
    if !fits_name {
        return code(234);
    }
    ctx.cpu.write_mem(name, &name_bytes)?;
    if !fits_data {
        return code(234);
    }
    if data != 0 {
        ctx.cpu.write_mem(data, &bytes)?;
    }
    code(0)
}
pub(super) fn query_info(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let key = ctx.arg_u32(0)?;
    let class = ctx.arg_u32(1)?;
    let class_size = ctx.arg_u32(2)?;
    let reserved = ctx.arg_u32(3)?;
    let outputs: [u32; 6] = [
        ctx.arg_u32(4)?,
        ctx.arg_u32(5)?,
        ctx.arg_u32(6)?,
        ctx.arg_u32(7)?,
        ctx.arg_u32(8)?,
        ctx.arg_u32(9)?,
    ];
    let security = ctx.arg_u32(10)?;
    let _ignored_time = ctx.arg_u32(11)?;
    let Some(path) = ctx.kernel.registry.path_for(key) else {
        return code(6);
    };
    if reserved != 0
        || security != 0
        || (class != 0 && !inout(ctx, class_size))
        || (class_size != 0 && !vfs_probe_output(ctx, class_size, 4))
        || outputs
            .iter()
            .any(|&ptr| ptr != 0 && !vfs_probe_output(ctx, ptr, 4))
    {
        return code(87);
    }
    if class != 0 && ctx.cpu.read_u32_le(class_size)? == 0 {
        return code(234);
    }
    if class != 0 && !vfs_probe_output(ctx, class, 2) {
        return code(87);
    }
    let children = ctx.kernel.registry.subkeys(&path);
    let values = ctx.kernel.registry.values(&path);
    let result = [
        children.len() as u32,
        children
            .iter()
            .map(|s| s.encode_utf16().count() as u32)
            .max()
            .unwrap_or(0),
        0,
        values.len() as u32,
        values
            .iter()
            .map(|(n, _)| n.encode_utf16().count() as u32)
            .max()
            .unwrap_or(0),
        values
            .iter()
            .map(|(_, v)| v.to_bytes().len() as u32)
            .max()
            .unwrap_or(0),
    ];
    for (ptr, value) in outputs.into_iter().zip(result) {
        if ptr != 0 {
            ctx.cpu.write_mem(ptr, &value.to_le_bytes())?;
        }
    }
    if class_size != 0 {
        ctx.cpu.write_mem(class_size, &0u32.to_le_bytes())?;
    }
    if class != 0 {
        ctx.cpu.write_mem(class, &[0, 0])?;
    }
    code(0)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{fresh_kernel, scanf_cpu, thunk_at};
    use super::*;
    use pocket_kernel::registry::RegistryValue;
    fn args(ctx: &mut CallCtx<'_>, values: &[u32]) {
        for (i, &v) in values.iter().enumerate() {
            if i < 4 {
                ctx.cpu
                    .write_reg([ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3][i], v)
                    .unwrap();
            } else {
                ctx.cpu
                    .write_mem(0x9000 + (i as u32 - 4) * 4, &v.to_le_bytes())
                    .unwrap();
            }
        }
    }
    #[test]
    fn registry_value_enumeration_handles_unicode_lengths_overflow_and_end() {
        let mut cpu = scanf_cpu();
        let mut kernel = fresh_kernel();
        let key = kernel.registry.create_and_open("HKCU\\Test");
        assert!(kernel.registry.set_value(
            "HKCU\\Test",
            "MiXeD🧭",
            RegistryValue::Sz("Été".into())
        ));
        let thunk = thunk_at(0x70000100);
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        let call = [key, 0, 0x1200, 0x1100, 0, 0x1108, 0x1300, 0x1104];
        ctx.cpu.write_mem(0x1200, &[0xaa; 32]).unwrap();
        ctx.cpu.write_mem(0x1300, &[0xbb; 16]).unwrap();
        ctx.cpu.write_mem(0x1100, &2u32.to_le_bytes()).unwrap();
        ctx.cpu.write_mem(0x1104, &16u32.to_le_bytes()).unwrap();
        args(&mut ctx, &call);
        assert_eq!(
            enum_value(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(234)
        );
        assert_eq!(ctx.cpu.read_u32_le(0x1100).unwrap(), 8); // seven UTF-16 units + NUL
        assert_eq!(ctx.cpu.read_mem(0x1200, 2).unwrap(), [0xaa; 2]);
        ctx.cpu.write_mem(0x1100, &8u32.to_le_bytes()).unwrap();
        ctx.cpu.write_mem(0x1104, &1u32.to_le_bytes()).unwrap();
        args(&mut ctx, &call);
        assert_eq!(
            enum_value(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(234)
        );
        assert_eq!(ctx.cpu.read_u32_le(0x1100).unwrap(), 7);
        assert_eq!(ctx.cpu.read_u32_le(0x1104).unwrap(), 8);
        assert_eq!(ctx.cpu.read_mem(0x1300, 2).unwrap(), [0xbb; 2]);
        ctx.cpu.write_mem(0x1100, &8u32.to_le_bytes()).unwrap();
        ctx.cpu.write_mem(0x1104, &8u32.to_le_bytes()).unwrap();
        args(&mut ctx, &call);
        assert_eq!(
            enum_value(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(ctx.cpu.read_u32_le(0x1108).unwrap(), 1);
        let data: Vec<_> = "Été"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(ctx.cpu.read_mem(0x1300, 8).unwrap(), data);
        let mut eof = call;
        eof[1] = 1;
        args(&mut ctx, &eof);
        assert_eq!(
            enum_value(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(259)
        );
        ctx.cpu.protect_region(0x1000, 4096, Prot::READ).unwrap();
        args(&mut ctx, &call);
        assert_eq!(
            enum_value(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(87)
        );
    }
    #[test]
    fn registry_query_info_obeys_twelve_argument_abi_and_real_counts() {
        let mut cpu = scanf_cpu();
        let mut kernel = fresh_kernel();
        let key = kernel.registry.create_and_open("HKCU\\Test");
        kernel.registry.create_key("HKCU\\Test\\Child");
        kernel
            .registry
            .set_value("HKCU\\Test", "Display", RegistryValue::Dword(42));
        kernel
            .registry
            .set_value("HKCU\\Test", "", RegistryValue::Binary(vec![0; 11]));
        let thunk = thunk_at(0x70000100);
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        args(
            &mut ctx,
            &[
                key, 0, 0, 0, 0x1100, 0x1104, 0x1108, 0x110c, 0x1110, 0x1114, 0, 0,
            ],
        );
        assert_eq!(
            query_info(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(0)
        );
        let actual: Vec<_> = (0..6)
            .map(|i| ctx.cpu.read_u32_le(0x1100 + i * 4).unwrap())
            .collect();
        assert_eq!(actual, [1, 5, 0, 2, 7, 11]);
        args(&mut ctx, &[key, 0, 0, 1, 0x1100, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            query_info(&mut ctx).unwrap(),
            DispatchOutcome::ReturnedR0(87)
        );
    }
}
