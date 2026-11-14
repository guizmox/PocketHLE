use super::tests::{fresh_kernel, lifecycle_test_module, scanf_cpu, thunk_at};
use super::*;
use pocket_cpu::Cpu;

#[test]
fn nested_module_release_keeps_outer_detach_resident_until_its_return() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let dependency = 0x30000000;
    let outer = 0x31000000;
    let helper = 0x32000000;
    for base in [dependency, outer, helper] {
        cpu.map_region(base, 0x2000, Prot::ALL).unwrap();
        let mut module = lifecycle_test_module(base, base + 0x1000);
        module.refcount = 1;
        module.resident_regions = vec![(base, 0x2000)];
        if base == outer {
            module.dependencies = vec![dependency];
        }
        if base == dependency {
            module.refcount = 0;
        }
        kernel.modules.push(module);
    }
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    ctx.cpu.write_reg(ArmReg::R0, outer).unwrap();
    ctx.cpu.write_reg(ArmReg::Lr, 0x1234).unwrap();
    assert_eq!(
        free_library(&mut ctx).unwrap(),
        DispatchOutcome::JumpTo(outer + 0x1000)
    );
    assert_eq!(ctx.kernel.module_attach_frames.len(), 1);

    // The outer DllMain has called FreeLibrary(helper) on its deeper stack.
    ctx.cpu.write_reg(ArmReg::Sp, 0x8ff0).unwrap();
    ctx.cpu.write_reg(ArmReg::Lr, outer + 0x1008).unwrap();
    ctx.cpu.write_reg(ArmReg::R0, helper).unwrap();
    assert_eq!(
        free_library(&mut ctx).unwrap(),
        DispatchOutcome::JumpTo(helper + 0x1000)
    );
    assert_eq!(ctx.kernel.module_attach_frames.len(), 2);
    ctx.cpu.write_reg(ArmReg::R0, 1).unwrap();
    assert_eq!(
        free_library(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(1)
    );
    assert_eq!(ctx.cpu.read_reg(ArmReg::Lr).unwrap(), outer + 0x1008);
    assert_eq!(ctx.cpu.read_reg(ArmReg::Sp).unwrap(), 0x8ff0);
    assert!(ctx.cpu.check_guest_access(helper, 4, Prot::EXEC).is_err());
    for base in [outer, dependency] {
        ctx.cpu.check_guest_access(base, 4, Prot::EXEC).unwrap();
    }
    assert_eq!(ctx.kernel.module_attach_frames.len(), 1);

    ctx.cpu.write_reg(ArmReg::Sp, 0x9000).unwrap();
    ctx.cpu.write_reg(ArmReg::R0, 1).unwrap();
    assert_eq!(
        free_library(&mut ctx).unwrap(),
        DispatchOutcome::JumpTo(dependency + 0x1000)
    );
    ctx.cpu.check_guest_access(outer, 4, Prot::EXEC).unwrap();
    ctx.cpu.write_reg(ArmReg::R0, 1).unwrap();
    assert_eq!(
        free_library(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(1)
    );
    assert!(ctx.kernel.modules.is_empty());
    assert!(ctx.kernel.module_attach_frames.is_empty());
    for base in [dependency, outer, helper] {
        assert!(ctx.cpu.check_guest_access(base, 4, Prot::EXEC).is_err());
    }
    assert_eq!(ctx.cpu.read_reg(ArmReg::Lr).unwrap(), 0x1234);
    assert_eq!(ctx.cpu.read_reg(ArmReg::Sp).unwrap(), 0x9000);
}
